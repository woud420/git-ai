#[allow(unused_imports)]
use super::*;
use crate::error::GitAiError;
use crate::model::domain::RewriteEvent;
use crate::operations::daemon::actor_coordinator_side_effects::RebaseMode;
use crate::operations::git::find_repository_in_path;
use std::path::Path;

impl ActorDaemonCoordinator {
    /// Handle a `CommitCreated` semantic event.
    ///
    /// `handled_revert_commits` is loop-carried across multiple `CommitCreated` events
    /// produced by a single `git revert A B`; callers must pass `&mut` and preserve
    /// event ordering.  `snapshots` is the per-actor timestamp map consumed by the
    /// async preflight.  The `continue` in the original loop becomes `return Ok(())`
    /// (handled at the call site via the event loop, not by the helper).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn handle_commit_created(
        &self,
        cmd: &crate::model::domain::NormalizedCommand,
        worktree: &str,
        base: Option<&str>,
        new_head: &str,
        rebase_mode: &RebaseMode,
        handled_revert_commits: &mut bool,
        commit_file_timestamp_snapshots: &mut CommitFileTimestampSnapshotHandles,
    ) -> Result<(), GitAiError> {
        let wt = Path::new(worktree);
        let mut handled_as_squash_merge = false;
        // DEFERRED (code-review #4): a pending `merge --squash` is
        // matched to the next commit by `base == pending.onto`
        // alone. If the user ABORTS the squash (e.g. `git reset
        // --hard` / `git checkout -- .`) and later makes an
        // unrelated commit on the same base, that commit is
        // mistaken for the squash and the source ref's session
        // metadata leaks into its note (inflating `git-ai stats`;
        // line-level blame stays correct). A robust fix is
        // non-trivial: the abandon commands (reset/checkout) are
        // not currently sequenced into this side-effect layer, so
        // we cannot clear the pending state on abort here, and a
        // metadata-prune alternative collides with the intentional
        // prompt-only-note feature. Left as-is pending one of
        // those two mechanisms.
        if !new_head.is_empty()
            && cmd.primary_command.as_deref() == Some("commit")
            && let Some(pending) = self.take_pending_squash_merge_for_worktree(wt)?
        {
            if base.is_some_and(|base| base == pending.onto) {
                let repo = find_repository_in_path(worktree)?;
                let outcome =
                    crate::operations::authorship::rewrite::handle_rewrite_event_with_metrics(
                        &repo,
                        RewriteEvent::SquashMerge {
                            source_head: pending.source_head,
                            squash_commit: new_head.to_string(),
                            onto: pending.onto,
                        },
                    )?;
                crate::operations::daemon::rewrite_metrics::spawn_rewrite_commit_metrics(
                    &repo,
                    outcome.metric_commits,
                );
                handled_as_squash_merge = true;
            } else {
                self.set_pending_squash_merge_for_worktree(wt, pending.source_head, pending.onto)?;
            }
        }

        if handled_as_squash_merge {
            // Squash authorship is reconstructed from the source ref captured
            // in sequenced trace/reflog state at `merge --squash` time.
        } else if rebase_mode.is_completing_rebase || rebase_mode.is_pull_rebase {
            // During rebase, note transfer is handled by non-FF detection.
            // Skip post-commit note generation to avoid overwriting shifted notes.
        } else if !new_head.is_empty() && cmd.primary_command.as_deref() == Some("revert") {
            if !*handled_revert_commits {
                // A single `git revert A B` creates one commit per source.
                // Reconstruct each destination from the matching HEAD transition
                // instead of treating the command as one final CommitCreated event.
                let repo = find_repository_in_path(worktree)?;
                let mut source_oids = cmd.revert_source_oids.clone();
                if source_oids.is_empty() {
                    source_oids = crate::operations::daemon::cherry_pick_helpers::resolve_explicit_revert_sources_for_side_effect(
                        &repo, cmd,
                    )?;
                }
                crate::operations::daemon::revert_rebase_helpers::apply_revert_complete_rewrite(
                    &repo,
                    cmd,
                    &source_oids,
                )?;
                *handled_revert_commits = true;
            }
        } else if !new_head.is_empty() {
            let repo = find_repository_in_path(worktree)?;
            // Collection is opt-in per repository: never generate new
            // authorship notes for repos outside allowed_repositories.
            // Preservation of pre-existing notes (cherry-pick rewrite
            // below) still runs.
            let repo_allowed = repo.is_collection_allowed(&crate::config::Config::fresh());
            let author = repo.effective_author_identity().formatted_or_unknown();
            let base_opt = base
                .map(ToOwned::to_owned)
                .filter(|b| !b.is_empty() && b != "initial");
            let recovery_file_timestamps =
                Self::take_commit_file_timestamps(commit_file_timestamp_snapshots, new_head).await;
            let recovery_preflight = |unknown_by_file: &crate::operations::authorship::attribution_recovery::UnknownLinesByFile| {
                self.wait_for_session_event_recovery_candidate(
                    &repo,
                    new_head,
                    recovery_file_timestamps.as_ref(),
                    unknown_by_file,
                );
            };

            // Post-commit note generation does synchronous git/filesystem work
            // and may briefly wait for transcript recovery. Mark it as blocking
            // so the transcript worker can process the recovery sweep promptly.
            if repo_allowed {
                run_blocking_side_effect(|| {
                    crate::operations::authorship::post_commit::post_commit_from_working_log_with_recovery_timestamps(
                        &repo,
                        base_opt.clone(),
                        new_head.to_string(),
                        author,
                        true,
                        recovery_file_timestamps.as_ref(),
                        Some(&recovery_preflight),
                    )
                })?;
            } else {
                tracing::debug!(
                    "skipping post-commit authorship: repository not in allowed_repositories"
                );
            }

            if cmd.primary_command.as_deref() == Some("commit")
                && let Some(pending) = self.take_pending_cherry_pick_no_commit_for_worktree(wt)?
            {
                if base.is_some_and(|base| base == pending.head) {
                    crate::operations::daemon::revert_rebase_helpers::apply_cherry_pick_no_commit_rewrite(
                        &repo,
                        &pending.source_commits,
                        &pending.head,
                        new_head,
                    )?;
                } else {
                    self.set_pending_cherry_pick_no_commit_for_worktree(
                        wt,
                        pending.source_commits,
                        pending.head,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Handle a `CommitAmended` semantic event.
    ///
    /// Returns `Ok(())` early (rather than `continue`) when the repo is denied and
    /// the old head carries no note, matching the original loop's `continue`.
    pub(crate) async fn handle_commit_amended(
        &self,
        worktree: &str,
        old_head: &str,
        new_head: &str,
        commit_file_timestamp_snapshots: &mut CommitFileTimestampSnapshotHandles,
    ) -> Result<(), GitAiError> {
        if old_head.is_empty()
            || new_head.is_empty()
            || old_head == new_head
            || !is_valid_oid(old_head)
            || is_zero_oid(old_head)
            || !is_valid_oid(new_head)
            || is_zero_oid(new_head)
        {
            return Ok(());
        }
        let repo = find_repository_in_path(worktree)?;
        // Collection is opt-in per repository. Amends still run when
        // the old head carries a note so existing attribution is
        // migrated (preservation), but denied repos never gain new
        // notes.
        let repo_allowed = repo.is_collection_allowed(&crate::config::Config::fresh());
        if !repo_allowed && crate::operations::git::notes_api::read_note(&repo, old_head).is_none()
        {
            tracing::debug!("skipping amend authorship: repository not in allowed_repositories");
            return Ok(());
        }
        let author = repo.effective_author_identity().formatted_or_unknown();
        let recovery_file_timestamps =
            Self::take_commit_file_timestamps(commit_file_timestamp_snapshots, new_head).await;
        let recovery_preflight = |unknown_by_file: &crate::operations::authorship::attribution_recovery::UnknownLinesByFile| {
            self.wait_for_session_event_recovery_candidate(
                &repo,
                new_head,
                recovery_file_timestamps.as_ref(),
                unknown_by_file,
            );
        };
        // Post-commit note generation does synchronous git/filesystem work
        // and may briefly wait for transcript recovery. Mark it as blocking
        // so the transcript worker can process the recovery sweep promptly.
        let amend_result = run_blocking_side_effect(|| {
            crate::operations::authorship::post_commit::post_commit_amend_with_recovery_timestamps_detailed(
                &repo,
                old_head,
                new_head,
                author,
                recovery_file_timestamps.as_ref(),
                Some(&recovery_preflight),
            )
        })?;
        if crate::operations::authorship::rewrite::rewrite_metrics_enabled() {
            crate::operations::daemon::rewrite_metrics::spawn_rewrite_commit_metrics(
                &repo,
                vec![
                    crate::operations::authorship::rewrite::RewriteMetricCommit::new(
                        new_head.to_string(),
                        vec![old_head.to_string()],
                        crate::operations::authorship::rewrite::RewriteMetricOperation::Amend,
                    )
                    .with_parent_sha(amend_result.parent_sha)
                    .with_authorship_note(amend_result.authorship_note),
                ],
            );
        }
        Ok(())
    }
}
