#[allow(unused_imports)]
use super::*;
use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use crate::operations::daemon::cherry_pick_helpers::rebase_new_tip_from_command;
use crate::operations::git::find_repository_in_path;
use crate::operations::git::oid::is_non_zero_oid;
use std::collections::HashMap;

impl ActorDaemonCoordinator {
    pub(crate) fn detect_and_handle_non_ff_rewrites(
        &self,
        cmd: &crate::model::domain::NormalizedCommand,
    ) -> Result<(), GitAiError> {
        let worktree = match cmd.worktree.as_ref() {
            Some(w) => w,
            None => return Ok(()),
        };

        let repo = find_repository_in_path(&worktree.to_string_lossy())?;

        // For rebase --skip/--continue that completes successfully, the trace2 data only shows
        // HEAD moving from onto → new_tip (a fast-forward). The real old_tip (original branch tip
        // before rebase started) was stored when the initial rebase failed. Use it here.
        let is_rebase_cmd = cmd.primary_command.as_deref() == Some("rebase");
        let pending_original_head = if is_rebase_cmd {
            self.take_pending_rebase_original_head_for_worktree(worktree)?
        } else {
            None
        };

        // Collect branch ref changes (skip notes, tags, etc.)
        let mut branch_changes: Vec<_> = cmd
            .ref_changes
            .iter()
            .filter(|rc| rc.reference.starts_with("refs/heads/"))
            .filter(|rc| is_non_zero_oid(&rc.old))
            .filter(|rc| is_non_zero_oid(&rc.new))
            .cloned()
            .collect();

        // If no branch ref changes found, fall back to HEAD changes (common for reset)
        if branch_changes.is_empty() {
            let head_changes: Vec<_> = cmd
                .ref_changes
                .iter()
                .filter(|rc| rc.reference == "HEAD")
                .filter(|rc| is_non_zero_oid(&rc.old))
                .filter(|rc| is_non_zero_oid(&rc.new))
                .cloned()
                .collect();
            if !head_changes.is_empty() {
                branch_changes = head_changes;
            }
        }

        if branch_changes.is_empty() && pending_original_head.is_none() {
            return Ok(());
        }

        // Collapse multiple changes to same branch: use (first old, last new)
        let mut collapsed: std::collections::HashMap<&str, (&str, &str)> =
            std::collections::HashMap::new();
        for rc in &branch_changes {
            collapsed
                .entry(rc.reference.as_str())
                .and_modify(|(_old, new)| *new = &rc.new)
                .or_insert((&rc.old, &rc.new));
        }

        // Extract "onto" hint from HEAD ref changes for rebases.
        // During a rebase, the first HEAD change target is the onto commit.
        let onto_hint: Option<String> = cmd
            .ref_changes
            .iter()
            .filter(|rc| rc.reference == "HEAD")
            .filter(|rc| is_non_zero_oid(&rc.new))
            .map(|rc| rc.new.clone())
            .next();

        // If we have a pending original head from a failed rebase, use it as old_tip
        // with the branch ref update as new_tip. This handles rebase --skip/--continue
        // where HEAD can contain extra checkout/detach movement that is not the
        // rebased branch tip.
        if let Some((original_head, stored_onto)) = pending_original_head
            && let Some(new_tip) = rebase_new_tip_from_command(cmd, &original_head)
        {
            if original_head != new_tip && !is_ancestor_commit(&repo, &original_head, &new_tip) {
                let command_rebase_onto =
                    rebase_onto_from_command(cmd, &repo, &original_head, &new_tip);
                let rebase_onto = stored_onto
                    .filter(|onto| {
                        onto != &original_head
                            && onto != &new_tip
                            && is_ancestor_commit(&repo, onto, &new_tip)
                    })
                    .or(command_rebase_onto);
                let outcome =
                    crate::operations::authorship::rewrite::handle_non_fast_forward_rewrite_with_operation(
                        &repo,
                        &original_head,
                        &new_tip,
                        rebase_onto.as_deref(),
                        crate::operations::authorship::rewrite::RewriteMetricOperation::Rebase,
                    )?;
                repo.storage.rename_working_log(&original_head, &new_tip)?;
                let conflict_base = rebase_onto.clone();
                let metric_context = process_conflict_resolution_working_logs(
                    &repo,
                    &new_tip,
                    conflict_base.as_deref(),
                )?;
                let metric_commits =
                    rewrite_metric_commits_with_context(outcome.metric_commits, metric_context);
                if !metric_commits.is_empty() {
                    let branch =
                        rewrite_metric_branch_for_transition(cmd, &original_head, &new_tip, None);
                    crate::operations::daemon::rewrite_metrics::spawn_rewrite_commit_metrics(
                        &repo,
                        rewrite_metric_commits_with_branch(metric_commits, branch),
                    );
                }
            }
            return Ok(());
        }

        for (reference, (old_tip, new_tip)) in &collapsed {
            if *old_tip == *new_tip {
                continue;
            }

            // Fast-forward — not a rewrite
            if is_ancestor_commit(&repo, old_tip, new_tip) {
                continue;
            }

            let rewrite_onto = if is_rebase_cmd {
                rebase_onto_from_command(cmd, &repo, old_tip, new_tip).or_else(|| onto_hint.clone())
            } else {
                onto_hint.clone()
            };
            let outcome = if is_rebase_cmd {
                crate::operations::authorship::rewrite::handle_non_fast_forward_rewrite_with_operation(
                    &repo,
                    old_tip,
                    new_tip,
                    rewrite_onto.as_deref(),
                    crate::operations::authorship::rewrite::RewriteMetricOperation::Rebase,
                )?
            } else if cmd.primary_command.as_deref() == Some("update-ref") {
                crate::operations::authorship::rewrite::handle_non_fast_forward_rewrite_with_operation(
                    &repo,
                    old_tip,
                    new_tip,
                    rewrite_onto.as_deref(),
                    crate::operations::authorship::rewrite::RewriteMetricOperation::UpdateRef,
                )?
            } else {
                crate::operations::authorship::rewrite::handle_non_fast_forward_rewrite_with_operation(
                    &repo,
                    old_tip,
                    new_tip,
                    rewrite_onto.as_deref(),
                    crate::operations::authorship::rewrite::RewriteMetricOperation::NonFastForward,
                )?
            };
            repo.storage.rename_working_log(old_tip, new_tip)?;
            let metric_context = if is_rebase_cmd {
                let conflict_base = rewrite_onto.clone().or_else(|| onto_hint.clone());
                process_conflict_resolution_working_logs(&repo, new_tip, conflict_base.as_deref())?
            } else {
                RewriteMetricContext::default()
            };
            let metric_commits =
                rewrite_metric_commits_with_context(outcome.metric_commits, metric_context);
            if !metric_commits.is_empty() {
                let branch =
                    rewrite_metric_branch_for_transition(cmd, old_tip, new_tip, Some(reference));
                crate::operations::daemon::rewrite_metrics::spawn_rewrite_commit_metrics(
                    &repo,
                    rewrite_metric_commits_with_branch(metric_commits, branch),
                );
            }
        }

        Ok(())
    }

    pub(crate) fn start_commit_file_timestamp_snapshots_for_command(
        command: &crate::model::domain::NormalizedCommand,
    ) -> CommitFileTimestampSnapshotHandles {
        let Some(worktree) = command.worktree.clone() else {
            return HashMap::new();
        };
        if command.exit_code != 0 || command.primary_command.as_deref() != Some("commit") {
            return HashMap::new();
        }

        let (_, new_head) = Self::resolve_heads_for_command(command);
        if !is_non_zero_oid(&new_head) {
            return HashMap::new();
        }

        let mut handles = HashMap::new();
        let task_commit_sha = new_head.clone();
        let handle = tokio::task::spawn_blocking(move || {
            match capture_commit_file_timestamps(&worktree, &task_commit_sha) {
                Ok(timestamps) => Some(timestamps),
                Err(error) => {
                    tracing::debug!(
                        %error,
                        commit_sha = %task_commit_sha,
                        "failed to capture commit-time file timestamps"
                    );
                    None
                }
            }
        });
        handles.insert(new_head, handle);

        handles
    }

    pub(crate) fn cache_commit_file_timestamp_snapshots_for_command(
        &self,
        command: &crate::model::domain::NormalizedCommand,
    ) -> Result<(), GitAiError> {
        let handles = Self::start_commit_file_timestamp_snapshots_for_command(command);
        if handles.is_empty() {
            return Ok(());
        }
        let mut cache = self
            .commit_file_timestamp_snapshots_by_root
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "commit file timestamp snapshot cache",
            })?;
        cache.insert(command.root_sid.clone(), handles);
        Ok(())
    }

    pub(crate) fn take_cached_commit_file_timestamp_snapshots(
        &self,
        root_sid: &str,
    ) -> Result<CommitFileTimestampSnapshotHandles, GitAiError> {
        let mut cache = self
            .commit_file_timestamp_snapshots_by_root
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "commit file timestamp snapshot cache",
            })?;
        Ok(cache.remove(root_sid).unwrap_or_default())
    }

    pub(crate) async fn take_commit_file_timestamps(
        handles: &mut CommitFileTimestampSnapshotHandles,
        commit_sha: &str,
    ) -> Option<crate::operations::authorship::attribution_recovery::FileTimestampsByPath> {
        let handle = handles.remove(commit_sha)?;
        match tokio::time::timeout(COMMIT_FILE_TIMESTAMP_SNAPSHOT_WAIT, handle).await {
            Ok(Ok(Some(timestamps))) if !timestamps.is_empty() => Some(timestamps),
            Ok(Ok(_)) => None,
            Ok(Err(error)) => {
                tracing::debug!(
                    %error,
                    %commit_sha,
                    "commit-time file timestamp task failed"
                );
                None
            }
            Err(_) => {
                tracing::debug!(
                    %commit_sha,
                    "commit-time file timestamp task timed out"
                );
                None
            }
        }
    }
}
