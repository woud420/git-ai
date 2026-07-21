#[allow(unused_imports)]
use super::*;
use crate::error::GitAiError;
use crate::operations::daemon::cherry_pick_helpers::{
    cherry_pick_command_has_flag, cherry_pick_destination_commits, cherry_pick_original_head,
    cherry_pick_source_args_for_side_effect, cherry_pick_state_exists_for_worktree,
    resolve_cherry_pick_source_args_with_git_in_head_context,
    resolve_explicit_cherry_pick_sources_for_side_effect,
    resolve_explicit_revert_sources_for_side_effect,
};
use crate::operations::daemon::revert_rebase_helpers::{
    apply_cherry_pick_complete_rewrite, apply_cherry_pick_no_commit_rewrite,
    apply_revert_complete_rewrite, strict_rebase_original_head_from_command,
};
use crate::operations::git::find_repository_in_path;
use std::time::Duration;

impl ActorDaemonCoordinator {
    pub(crate) async fn maybe_apply_side_effects_for_applied_command(
        &self,
        family: Option<&str>,
        applied: &crate::model::domain::AppliedCommand,
        commit_file_timestamp_snapshots: &mut CommitFileTimestampSnapshotHandles,
    ) -> Result<(), GitAiError> {
        // Test-only: allow inducing a panic in the side-effect pipeline to verify
        // that the daemon's catch_unwind recovery keeps the process alive.
        // Uses a file-based flag so the test can remove the file between commands.
        #[cfg(feature = "test-support")]
        if let Ok(path) = std::env::var("GIT_AI_TEST_PANIC_IN_SIDE_EFFECT_FLAG")
            && std::path::Path::new(&path).exists()
        {
            panic!("test-induced panic in side-effect pipeline");
        }

        let cmd = &applied.command;
        let events = &applied.analysis.events;

        let primary = cmd.primary_command.as_deref().unwrap_or("unknown");

        #[cfg(feature = "test-support")]
        if let Ok(spec) = std::env::var("GIT_AI_TEST_DELAY_SIDE_EFFECT_MS_FOR_COMMAND") {
            for entry in spec.split(',') {
                let Some((command, delay_ms)) = entry.split_once('=') else {
                    continue;
                };
                if command == primary
                    && let Ok(delay_ms) = delay_ms.parse::<u64>()
                    && delay_ms > 0
                {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    break;
                }
            }
        }

        let is_write_op = matches!(
            primary,
            "commit"
                | "rebase"
                | "merge"
                | "cherry-pick"
                | "am"
                | "stash"
                | "reset"
                | "push"
                | "update-ref"
        );
        if is_write_op && cmd.exit_code == 0 {
            let repo_path = cmd
                .worktree
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let post_head = cmd
                .ref_changes
                .iter()
                .rev()
                .find(|change| change.reference == "HEAD")
                .map(|change| change.new.clone())
                .unwrap_or_default();
            tracing::info!(
                op = primary,
                repo = %repo_path,
                new_head = %post_head,
                "git write op completed"
            );
        }

        let saw_pull_event = events.iter().any(|event| {
            matches!(
                event,
                crate::model::domain::SemanticEvent::PullCompleted { .. }
            )
        });
        let pull_uses_rebase = events.iter().any(|event| {
            matches!(
                event,
                crate::model::domain::SemanticEvent::PullCompleted {
                    strategy: crate::model::domain::PullStrategy::Rebase
                        | crate::model::domain::PullStrategy::RebaseMerges,
                    ..
                }
            )
        });
        if std::env::var("GIT_AI_DEBUG_DAEMON_TRACE")
            .ok()
            .as_deref()
            .is_some_and(|v| v == "1")
        {
            tracing::debug!(
                command = cmd.invoked_command.clone().unwrap_or_default(),
                primary = cmd.primary_command.clone().unwrap_or_default(),
                seq = applied.seq,
                argv = ?cmd.raw_argv,
                invoked_args = ?cmd.invoked_args,
                ref_changes_len = cmd.ref_changes.len(),
                ref_changes = ?cmd.ref_changes,
                events = ?events,
                exit_code = cmd.exit_code,
                "side-effect trace"
            );
        }
        // Non-FF rewrite detection: fires for commands that rewrite history via ref moves.
        // Skip for: checkout/switch/branch (no rewriting), cherry-pick (handled separately),
        // and plain commit/amend (CommitCreated/CommitAmended events handle those).
        // Do NOT skip for rebase — the CommitCreated events during rebase are intermediate
        // replayed commits; note transfer happens via non-FF detection on the final ref move.
        // But DO skip for rebase --abort, which restores state instead of finishing a rewrite.
        let is_rebase = cmd.primary_command.as_deref() == Some("rebase");
        let is_rebase_abort = is_rebase && cmd.invoked_args.iter().any(|a| a == "--abort");
        let is_completing_rebase = is_rebase && !is_rebase_abort;
        let is_pull_rebase = pull_uses_rebase && cmd.primary_command.as_deref() == Some("pull");
        let skip_non_ff = if is_completing_rebase || is_pull_rebase {
            false
        } else if is_rebase_abort {
            if let Some(worktree) = cmd.worktree.as_ref() {
                self.clear_pending_rebase_original_head_for_worktree(worktree)?;
            }
            true
        } else {
            events.iter().any(|event| {
                matches!(
                    event,
                    crate::model::domain::SemanticEvent::CommitAmended { .. }
                        | crate::model::domain::SemanticEvent::CommitCreated { .. }
                        | crate::model::domain::SemanticEvent::CherryPickComplete { .. }
                        | crate::model::domain::SemanticEvent::Reset { .. }
                )
            }) || matches!(
                cmd.primary_command.as_deref(),
                Some("checkout" | "switch" | "branch" | "stash")
            )
        };
        if !skip_non_ff && cmd.exit_code == 0 {
            self.detect_and_handle_non_ff_rewrites(cmd)?;
        }

        if cmd.exit_code != 0 {
            let rebase_start = cmd
                .ref_changes
                .iter()
                .find(|change| {
                    change.reference == "HEAD"
                        && is_valid_oid(&change.old)
                        && !is_zero_oid(&change.old)
                        && is_valid_oid(&change.new)
                        && !is_zero_oid(&change.new)
                })
                .map(|change| (change.old.clone(), change.new.clone()));
            let pull_has_rebase_start =
                cmd.primary_command.as_deref() == Some("pull") && rebase_start.is_some();
            let is_rebase_like = cmd.primary_command.as_deref() == Some("rebase")
                || (cmd.primary_command.as_deref() == Some("pull")
                    && (pull_uses_rebase || pull_has_rebase_start));
            if is_rebase_like {
                let worktree = cmd.worktree.as_ref().ok_or_else(|| {
                    GitAiError::Generic(format!(
                        "rebase side-effect state requires worktree sid={}",
                        cmd.root_sid
                    ))
                })?;
                if cmd.invoked_args.iter().any(|arg| arg == "--abort") {
                    self.clear_pending_rebase_original_head_for_worktree(worktree)?;
                } else if cmd.exit_code != 0 && !rebase_is_control_mode(cmd) {
                    let semantic_old_head = rebase_start
                        .as_ref()
                        .map(|(old, _)| old.as_str())
                        .unwrap_or("");
                    let pending_old_head =
                        strict_rebase_original_head_from_command(cmd, semantic_old_head);
                    if let Some(old_head) = pending_old_head {
                        let rebase_onto = rebase_start.as_ref().map(|(_, new)| new.clone());
                        if std::env::var("GIT_AI_DEBUG_DAEMON_TRACE")
                            .ok()
                            .as_deref()
                            .is_some_and(|v| v == "1")
                        {
                            tracing::debug!(
                                ?family,
                                %old_head,
                                ?rebase_onto,
                                "pending rebase original head set"
                            );
                        }
                        self.set_pending_rebase_original_head_for_worktree(
                            worktree,
                            old_head,
                            rebase_onto,
                        )?;
                    }
                }
            }
            if cmd.primary_command.as_deref() == Some("cherry-pick") {
                let worktree = cmd.worktree.as_ref().ok_or_else(|| {
                    GitAiError::Generic(format!(
                        "cherry-pick side-effect state requires worktree sid={}",
                        cmd.root_sid
                    ))
                })?;
                if cmd.invoked_args.iter().any(|arg| arg == "--abort") {
                    self.clear_pending_cherry_pick_sources_for_worktree(worktree)?;
                    self.clear_pending_cherry_pick_no_commit_for_worktree(worktree)?;
                } else if cmd.exit_code != 0 {
                    let new_commits = cherry_pick_destination_commits(cmd);
                    let is_continue = cherry_pick_command_has_flag(cmd, "--continue");
                    let is_skip = cherry_pick_command_has_flag(cmd, "--skip");
                    let mut source_oids = cmd.cherry_pick_source_oids.clone();
                    let mut source_oids_from_daemon_pending = false;
                    if source_oids.is_empty()
                        && (!new_commits.is_empty()
                            || cherry_pick_state_exists_for_worktree(worktree))
                    {
                        let repo = find_repository_in_path(&worktree.to_string_lossy())?;
                        source_oids =
                            resolve_explicit_cherry_pick_sources_for_side_effect(&repo, cmd)?;
                    }
                    if source_oids.is_empty() && (is_continue || is_skip) {
                        source_oids = self.pending_cherry_pick_sources_for_worktree(worktree)?;
                        source_oids_from_daemon_pending = !source_oids.is_empty();
                    }
                    let skipped_sources = usize::from(is_skip && source_oids_from_daemon_pending);
                    let applied_source_oids = source_oids
                        .iter()
                        .skip(skipped_sources)
                        .cloned()
                        .collect::<Vec<_>>();
                    if !new_commits.is_empty() && !applied_source_oids.is_empty() {
                        let repo = find_repository_in_path(&worktree.to_string_lossy())?;
                        let original_head = cherry_pick_original_head(cmd).ok_or_else(|| {
                            GitAiError::Generic(format!(
                                "cherry-pick completed commits without original HEAD sid={}",
                                cmd.root_sid
                            ))
                        })?;
                        apply_cherry_pick_complete_rewrite(
                            &repo,
                            &original_head,
                            &applied_source_oids,
                            &new_commits,
                        )?;
                    }
                    if !source_oids.is_empty() || is_continue || is_skip {
                        let applied_sources = new_commits
                            .len()
                            .min(source_oids.len().saturating_sub(skipped_sources));
                        let consumed_sources = skipped_sources + applied_sources;
                        let remaining = source_oids
                            .iter()
                            .skip(consumed_sources.min(source_oids.len()))
                            .cloned()
                            .collect();
                        self.set_pending_cherry_pick_sources_for_worktree(worktree, remaining)?;
                    }
                }
            }
            // Fix #957: `checkout/switch --merge` exits with code 1 when it produces
            // conflict markers but HEAD still moves to the target branch.  We must not
            // return early here — fall through so apply_checkout_switch_working_log_side_effect
            // and recent_checkout_switch_prerequisite_from_command can migrate the working log.
            let is_merge_checkout =
                matches!(cmd.primary_command.as_deref(), Some("checkout" | "switch")) && {
                    let p = parsed_invocation_for_normalized_command(cmd);
                    p.has_command_flag("--merge") || p.has_command_flag("-m")
                };
            // For stash pop/apply/branch with non-zero exit (typically conflict), don't
            // skip processing. The stash may have been partially applied and attribution
            // should still be restored. We cannot rely on `has_stash_conflict_for_repo`
            // because in daemon mode the conflict check runs lazily at sync time -- by
            // which point the user may already have resolved the conflict with `git add`.
            // Instead, always attempt restoration for stash restore operations; if the
            // stash was never applied the restore is a harmless no-op.
            let is_stash_restore = cmd.primary_command.as_deref() == Some("stash")
                && events.iter().any(|event| {
                    matches!(
                        event,
                        crate::model::domain::SemanticEvent::StashOperation {
                            kind: crate::model::domain::StashOpKind::Pop
                                | crate::model::domain::StashOpKind::Apply
                                | crate::model::domain::StashOpKind::Branch,
                            ..
                        }
                    )
                });
            let is_merge_squash = cmd.primary_command.as_deref() == Some("merge")
                && events.iter().any(|event| {
                    matches!(
                        event,
                        crate::model::domain::SemanticEvent::MergeSquash { .. }
                    )
                });
            if !is_merge_checkout && !is_stash_restore && !is_merge_squash {
                return Ok(());
            }
            if is_stash_restore {
                tracing::debug!(
                    sid = %cmd.root_sid,
                    "stash restore with non-zero exit, continuing to restore attribution"
                );
            }
        }

        if let Some(worktree) = cmd.worktree.as_ref() {
            let worktree = worktree.to_string_lossy().to_string();
            let mut handled_revert_commits = false;
            for event in events {
                match event {
                    crate::model::domain::SemanticEvent::CloneCompleted { .. } => {
                        apply_clone_notes_sync_side_effect(&worktree)?;
                    }
                    crate::model::domain::SemanticEvent::PullCompleted { .. } => {
                        apply_pull_notes_sync_side_effect(
                            &worktree,
                            cmd.invoked_command.as_deref(),
                            &cmd.invoked_args,
                        )?;
                    }
                    crate::model::domain::SemanticEvent::PushCompleted { .. } => {
                        apply_push_side_effect(
                            &worktree,
                            cmd.invoked_command.as_deref(),
                            &cmd.invoked_args,
                        )?;
                    }
                    crate::model::domain::SemanticEvent::CherryPickComplete {
                        original_head,
                        new_head,
                        source_commits,
                        new_commits,
                    } => {
                        if !new_head.is_empty() {
                            let repo = find_repository_in_path(&worktree)?;
                            let mut sources = source_commits.clone();
                            let is_skip = cherry_pick_command_has_flag(cmd, "--skip");
                            let explicit_source_args = cherry_pick_source_args_for_side_effect(cmd);
                            if !sources.is_empty() {
                                self.clear_pending_cherry_pick_sources_for_worktree(
                                    worktree.as_ref(),
                                )?;
                            } else if !explicit_source_args.is_empty() {
                                let head_context =
                                    (!original_head.is_empty()).then_some(original_head.as_str());
                                sources = resolve_cherry_pick_source_args_with_git_in_head_context(
                                    &repo,
                                    &explicit_source_args,
                                    head_context,
                                )?;
                                self.clear_pending_cherry_pick_sources_for_worktree(
                                    worktree.as_ref(),
                                )?;
                            } else {
                                sources = self.take_pending_cherry_pick_sources_for_worktree(
                                    worktree.as_ref(),
                                )?;
                                if is_skip && !sources.is_empty() {
                                    sources.remove(0);
                                }
                            }
                            let destinations = if new_commits.is_empty() {
                                vec![new_head.clone()]
                            } else {
                                new_commits.clone()
                            };
                            if original_head != new_head {
                                if original_head.is_empty() {
                                    return Err(GitAiError::Generic(format!(
                                        "cherry-pick complete missing original HEAD sid={}",
                                        cmd.root_sid
                                    )));
                                }
                                apply_cherry_pick_complete_rewrite(
                                    &repo,
                                    original_head,
                                    &sources,
                                    &destinations,
                                )?;
                            }
                        }
                    }
                    crate::model::domain::SemanticEvent::CherryPickNoCommit {
                        source_commits,
                        head,
                    } => {
                        let mut sources = source_commits.clone();
                        if sources.is_empty() {
                            let repo = find_repository_in_path(&worktree)?;
                            sources =
                                resolve_explicit_cherry_pick_sources_for_side_effect(&repo, cmd)?;
                        }
                        if !head.is_empty() && !sources.is_empty() {
                            self.set_pending_cherry_pick_no_commit_for_worktree(
                                worktree.as_ref(),
                                sources,
                                head.clone(),
                            )?;
                        }
                    }
                    crate::model::domain::SemanticEvent::MergeSquash { source_head, onto } => {
                        self.set_pending_squash_merge_for_worktree(
                            worktree.as_ref(),
                            source_head.clone(),
                            onto.clone(),
                        )?;
                    }
                    crate::model::domain::SemanticEvent::StashOperation { kind, head } => {
                        let repo = find_repository_in_path(&worktree)?;
                        match kind {
                            crate::model::domain::StashOpKind::Push
                            | crate::model::domain::StashOpKind::Unknown => {
                                let resolved_stash =
                                    cmd.stash_target_oid.as_deref().or_else(|| {
                                        cmd.ref_changes
                                        .iter()
                                        .find(|rc| rc.reference == "refs/stash")
                                        .map(|rc| rc.new.as_str())
                                        .filter(|s| {
                                            !s.is_empty()
                                                && *s != "0000000000000000000000000000000000000000"
                                        })
                                    });
                                if let Some(stash_sha) = resolved_stash {
                                    let push_head =
                                        stash_base_head(&repo, stash_sha).or_else(|| head.clone());
                                    if let Some(head_sha) = push_head.as_deref() {
                                        let pathspecs = Self::stash_pathspecs_from_command(cmd);
                                        crate::operations::authorship::rewrite_stash::handle_stash_create(
                                            &repo, stash_sha, head_sha, pathspecs,
                                        )?;
                                    }
                                }
                            }
                            crate::model::domain::StashOpKind::Pop => {
                                if let Some(stash_sha) = resolve_stash_sha(cmd) {
                                    let base_head = stash_base_head(&repo, stash_sha);
                                    let target_head = head.as_deref().or(base_head.as_deref());
                                    crate::operations::authorship::rewrite_stash::handle_stash_pop_or_apply_with_head(
                                        &repo, stash_sha, true, target_head,
                                    )?;
                                }
                            }
                            crate::model::domain::StashOpKind::Apply
                            | crate::model::domain::StashOpKind::Branch => {
                                if let Some(stash_sha) = resolve_stash_sha(cmd) {
                                    let effective_head = if matches!(
                                        kind,
                                        crate::model::domain::StashOpKind::Branch
                                    ) {
                                        stash_base_head(&repo, stash_sha)
                                    } else {
                                        None
                                    };
                                    let base_head = stash_base_head(&repo, stash_sha);
                                    let target_head = effective_head
                                        .as_deref()
                                        .or(head.as_deref())
                                        .or(base_head.as_deref());
                                    crate::operations::authorship::rewrite_stash::handle_stash_pop_or_apply_with_head(
                                        &repo, stash_sha, false, target_head,
                                    )?;
                                }
                            }
                            crate::model::domain::StashOpKind::Drop => {
                                if let Some(stash_sha) = resolve_stash_sha(cmd) {
                                    crate::operations::authorship::rewrite_stash::handle_stash_drop(
                                        &repo, stash_sha,
                                    )?;
                                }
                            }
                            _ => {}
                        }
                    }
                    crate::model::domain::SemanticEvent::CommitCreated { base, new_head } => {
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
                            && let Some(pending) =
                                self.take_pending_squash_merge_for_worktree(worktree.as_ref())?
                        {
                            if base.as_deref().is_some_and(|base| base == pending.onto) {
                                let repo = find_repository_in_path(&worktree)?;
                                let outcome =
                                    crate::operations::authorship::rewrite::handle_rewrite_event_with_metrics(
                                        &repo,
                                        crate::operations::authorship::rewrite::RewriteEvent::SquashMerge {
                                            source_head: pending.source_head,
                                            squash_commit: new_head.clone(),
                                            onto: pending.onto,
                                        },
                                    )?;
                                crate::operations::daemon::rewrite_metrics::spawn_rewrite_commit_metrics(
                                    &repo,
                                    outcome.metric_commits,
                                );
                                handled_as_squash_merge = true;
                            } else {
                                self.set_pending_squash_merge_for_worktree(
                                    worktree.as_ref(),
                                    pending.source_head,
                                    pending.onto,
                                )?;
                            }
                        }

                        if handled_as_squash_merge {
                            // Squash authorship is reconstructed from the source ref captured
                            // in sequenced trace/reflog state at `merge --squash` time.
                        } else if is_completing_rebase || is_pull_rebase {
                            // During rebase, note transfer is handled by non-FF detection.
                            // Skip post-commit note generation to avoid overwriting shifted notes.
                        } else if !new_head.is_empty()
                            && cmd.primary_command.as_deref() == Some("revert")
                        {
                            if !handled_revert_commits {
                                // A single `git revert A B` creates one commit per source.
                                // Reconstruct each destination from the matching HEAD transition
                                // instead of treating the command as one final CommitCreated event.
                                let repo = find_repository_in_path(&worktree)?;
                                let mut source_oids = cmd.revert_source_oids.clone();
                                if source_oids.is_empty() {
                                    source_oids = resolve_explicit_revert_sources_for_side_effect(
                                        &repo, cmd,
                                    )?;
                                }
                                apply_revert_complete_rewrite(&repo, cmd, &source_oids)?;
                                handled_revert_commits = true;
                            }
                        } else if !new_head.is_empty() {
                            let repo = find_repository_in_path(&worktree)?;
                            // Collection is opt-in per repository: never generate new
                            // authorship notes for repos outside allowed_repositories.
                            // Preservation of pre-existing notes (cherry-pick rewrite
                            // below) still runs.
                            let repo_allowed =
                                repo.is_collection_allowed(&crate::config::Config::fresh());
                            let author = repo.effective_author_identity().formatted_or_unknown();
                            let base_opt = base.clone().filter(|b| !b.is_empty() && b != "initial");
                            let recovery_file_timestamps = Self::take_commit_file_timestamps(
                                commit_file_timestamp_snapshots,
                                new_head,
                            )
                            .await;
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
                                        new_head.clone(),
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
                                && let Some(pending) = self
                                    .take_pending_cherry_pick_no_commit_for_worktree(
                                        worktree.as_ref(),
                                    )?
                            {
                                if base.as_deref().is_some_and(|base| base == pending.head) {
                                    apply_cherry_pick_no_commit_rewrite(
                                        &repo,
                                        &pending.source_commits,
                                        &pending.head,
                                        new_head,
                                    )?;
                                } else {
                                    self.set_pending_cherry_pick_no_commit_for_worktree(
                                        worktree.as_ref(),
                                        pending.source_commits,
                                        pending.head,
                                    )?;
                                }
                            }
                        }
                    }
                    crate::model::domain::SemanticEvent::CommitAmended { old_head, new_head } => {
                        if !old_head.is_empty()
                            && !new_head.is_empty()
                            && old_head != new_head
                            && is_valid_oid(old_head)
                            && !is_zero_oid(old_head)
                            && is_valid_oid(new_head)
                            && !is_zero_oid(new_head)
                        {
                            let repo = find_repository_in_path(&worktree)?;
                            // Collection is opt-in per repository. Amends still run when
                            // the old head carries a note so existing attribution is
                            // migrated (preservation), but denied repos never gain new
                            // notes.
                            let repo_allowed =
                                repo.is_collection_allowed(&crate::config::Config::fresh());
                            if !repo_allowed
                                && crate::operations::git::notes_api::read_note(&repo, old_head)
                                    .is_none()
                            {
                                tracing::debug!(
                                    "skipping amend authorship: repository not in allowed_repositories"
                                );
                                continue;
                            }
                            let author = repo.effective_author_identity().formatted_or_unknown();
                            let recovery_file_timestamps = Self::take_commit_file_timestamps(
                                commit_file_timestamp_snapshots,
                                new_head,
                            )
                            .await;
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
                        }
                    }
                    crate::model::domain::SemanticEvent::Reset {
                        kind,
                        old_head,
                        new_head,
                    } if !old_head.is_empty() && !new_head.is_empty() && old_head != new_head => {
                        let repo = find_repository_in_path(&worktree)?;
                        match kind {
                            crate::model::domain::ResetKind::Hard => {
                                repo.storage.delete_working_log_for_base_commit(old_head)?;
                            }
                            _ => {
                                if is_ancestor_commit(&repo, new_head, old_head) {
                                    crate::operations::authorship::rewrite_reset::reconstruct_working_log_after_backward_reset(
                                        &repo, old_head, new_head,
                                    )?;
                                } else if !is_ancestor_commit(&repo, old_head, new_head) {
                                    let outcome =
                                        crate::operations::authorship::rewrite::handle_rewrite_event_with_metrics(
                                        &repo,
                                        crate::operations::authorship::rewrite::RewriteEvent::NonFastForward {
                                            old_tip: old_head.to_string(),
                                            new_tip: new_head.to_string(),
                                            onto: None,
                                        },
                                    )?;
                                    crate::operations::daemon::rewrite_metrics::spawn_rewrite_commit_metrics(
                                        &repo,
                                        outcome.metric_commits,
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if matches!(cmd.primary_command.as_deref(), Some("checkout" | "switch")) {
            if let Some(prerequisite) = recent_checkout_switch_prerequisite_from_command(cmd) {
                let family = family.map(std::borrow::ToOwned::to_owned).or_else(|| {
                    cmd.worktree.as_ref().and_then(|worktree| {
                        find_repository_in_path(&worktree.to_string_lossy())
                            .ok()
                            .map(|repo| family_key_for_repository(&repo))
                    })
                });
                if let Some(family) = family {
                    self.record_recent_replay_prerequisite(&family, prerequisite)?;
                }
            }
            apply_checkout_switch_working_log_side_effect(cmd)?;
        }

        if saw_pull_event && let Some(worktree) = cmd.worktree.as_ref() {
            let (old_head, new_head) = Self::resolve_heads_for_command(cmd);
            if !old_head.is_empty() && !new_head.is_empty() && old_head != new_head {
                let repo = find_repository_in_path(&worktree.to_string_lossy())?;
                if repo_is_ancestor(&repo, &old_head, &new_head) {
                    apply_pull_fast_forward_working_log_side_effect(
                        &worktree.to_string_lossy(),
                        &old_head,
                        &new_head,
                    )?;
                }
            }
        }

        // Handle update-ref: migrate working logs and authorship notes when the ref
        // update affects the currently checked-out branch.
        if primary == "update-ref"
            && let Some(worktree) = cmd.worktree.as_ref()
        {
            for event in events {
                if let crate::model::domain::SemanticEvent::RefUpdated {
                    reference,
                    old,
                    new,
                } = event
                {
                    if reference != "HEAD" && !reference.starts_with("refs/heads/")
                        || !is_valid_oid(old)
                        || is_zero_oid(old)
                        || !is_valid_oid(new)
                        || is_zero_oid(new)
                        || old == new
                    {
                        continue;
                    }
                    let repo = find_repository_in_path(&worktree.to_string_lossy())?;
                    if repo_is_ancestor(&repo, old, new) {
                        let affects_checked_out_branch = reference == "HEAD"
                            || cmd.ref_changes.iter().any(|change| {
                                change.reference == "HEAD"
                                    && change.old == *old
                                    && change.new == *new
                            });
                        if affects_checked_out_branch {
                            if repo.storage.has_working_log(old) {
                                let author =
                                    repo.effective_author_identity().formatted_or_unknown();
                                crate::operations::authorship::post_commit::post_commit_from_working_log(
                                    &repo,
                                    Some(old.to_string()),
                                    new.to_string(),
                                    author,
                                    true,
                                )?;
                            }
                            repo.storage.rename_working_log(old, new)?;
                        }
                    } else {
                        crate::operations::authorship::rewrite::handle_rewrite_event(
                            &repo,
                            crate::operations::authorship::rewrite::RewriteEvent::NonFastForward {
                                old_tip: old.to_string(),
                                new_tip: new.to_string(),
                                onto: None,
                            },
                        )?;
                    }
                }
            }
        }

        let parsed_invocation = parsed_invocation_for_normalized_command(cmd);
        for trigger in transcript_sweep_triggers_for_events(events) {
            if trigger == crate::operations::daemon::stream_worker::SweepTrigger::PostPush
                && crate::operations::git::cli_parser::is_dry_run(&parsed_invocation.command_args)
            {
                tracing::debug!("transcript sweep trigger skipped for dry-run push");
                continue;
            }
            self.trigger_transcript_sweep(trigger);
        }

        Ok(())
    }
}
