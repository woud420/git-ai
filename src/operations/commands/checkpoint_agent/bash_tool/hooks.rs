//! Pre- and post-tool-use hook orchestration for bash-tool stat-diff attribution.

use crate::error::GitAiError;
use crate::model::daemon_control::ControlRequest;
use crate::model::working_log::AgentId;
use crate::operations::daemon::send_control_request;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::daemon_api::{
    BashSessionEndSignal, BashToolHookContext, DaemonWatermarks, effective_daemon_socket,
    query_daemon_bash_snapshot, query_daemon_watermarks, signal_daemon_bash_session_end,
};
use super::effective_hook_timeout_ms;
use super::path_filter::git_index_mtime_ns;
use super::snapshot::{diff, git_status_fallback, snapshot};
use super::types::{BashCheckpointAction, BashPostHookResult, BashPreHookResult};

// ---------------------------------------------------------------------------
// Pre-hook
// ---------------------------------------------------------------------------

/// Handle the pre-tool-use hook with full agent context.
///
/// Takes a filesystem snapshot and sends it to the daemon via `BashSessionStart`.
/// The daemon stores the snapshot in memory for retrieval at post-hook time.
pub fn handle_bash_pre_tool_use_with_context(
    repo_root: &Path,
    session_id: &str,
    tool_use_id: &str,
    agent_id: &AgentId,
    agent_metadata: Option<&HashMap<String, String>>,
    trace_id: &str,
    command: Option<&str>,
) -> Result<BashPreHookResult, GitAiError> {
    handle_bash_pre_tool_use_with_context_and_cwd(
        repo_root,
        repo_root,
        BashToolHookContext {
            session_id,
            tool_use_id,
            agent_id,
            agent_metadata,
            trace_id,
            command,
        },
    )
}

/// Handle the pre-tool-use hook with a separately tracked original cwd.
pub fn handle_bash_pre_tool_use_with_context_and_cwd(
    repo_root: &Path,
    original_cwd: &Path,
    context: BashToolHookContext<'_>,
) -> Result<BashPreHookResult, GitAiError> {
    let started_at_ns = crate::model::repository::bash_history_db::unix_time_ns();
    let repo_working_dir = repo_root.to_string_lossy().to_string();

    let wm = query_daemon_watermarks(&repo_working_dir).or_else(|| {
        git_index_mtime_ns(repo_root).map(|ts| DaemonWatermarks {
            per_file: HashMap::new(),
            worktree: Some(ts),
        })
    });
    let snap = snapshot(
        repo_root,
        context.session_id,
        context.tool_use_id,
        wm.as_ref(),
    )?;

    // When watermarks are unavailable (no daemon + no .git/index), the snapshot
    // contains every non-ignored file in the repo. Using that as dirty_paths
    // would trigger per-file repo discovery + file reads in
    // build_checkpoint_files — catastrophic on large repos. Fall back to git
    // status which only reports actually changed files.
    let dirty_paths: Vec<PathBuf> = if wm.is_none() {
        match git_status_fallback(repo_root) {
            Ok(paths) => paths.into_iter().map(|p| repo_root.join(p)).collect(),
            Err(_) => vec![],
        }
    } else {
        snap.entries.keys().map(|rel| repo_root.join(rel)).collect()
    };

    let socket = effective_daemon_socket().ok_or_else(|| {
        GitAiError::Generic("no daemon socket available for BashSessionStart".into())
    })?;

    let request = ControlRequest::BashSessionStart {
        repo_work_dir: repo_working_dir,
        original_cwd: Some(original_cwd.to_string_lossy().to_string()),
        session_id: context.session_id.to_string(),
        tool_use_id: context.tool_use_id.to_string(),
        agent_id: context.agent_id.clone(),
        metadata: context.agent_metadata.cloned().unwrap_or_default(),
        stat_snapshot: Box::new(snap),
        trace_id: context.trace_id.to_string(),
        started_at_ns,
        command: context.command.map(ToString::to_string),
    };

    send_control_request(&socket, &request)?;

    Ok(BashPreHookResult { dirty_paths })
}

// ---------------------------------------------------------------------------
// Post-hook
// ---------------------------------------------------------------------------

/// Handle the post-tool-use hook for a bash tool invocation.
///
/// Queries the daemon for the pre-snapshot (stored during `BashSessionStart`),
/// takes a post-snapshot, diffs them, signals `BashSessionEnd`, and returns
/// the list of changed files.
pub fn handle_bash_post_tool_use(
    repo_root: &Path,
    session_id: &str,
    tool_use_id: &str,
    agent_id: &AgentId,
    agent_metadata: Option<&HashMap<String, String>>,
    trace_id: &str,
    command: Option<&str>,
) -> Result<BashPostHookResult, GitAiError> {
    handle_bash_post_tool_use_with_cwd(
        repo_root,
        repo_root,
        BashToolHookContext {
            session_id,
            tool_use_id,
            agent_id,
            agent_metadata,
            trace_id,
            command,
        },
    )
}

/// Handle the post-tool-use hook with a separately tracked original cwd.
pub fn handle_bash_post_tool_use_with_cwd(
    repo_root: &Path,
    original_cwd: &Path,
    context: BashToolHookContext<'_>,
) -> Result<BashPostHookResult, GitAiError> {
    let invocation_key = format!("{}:{}", context.session_id, context.tool_use_id);

    let hook_start = Instant::now();
    let ended_at_ns = crate::model::repository::bash_history_db::unix_time_ns();
    let hook_timeout = Duration::from_millis(effective_hook_timeout_ms());
    let repo_working_dir = repo_root.to_string_lossy().to_string();
    let metadata = context.agent_metadata.cloned().unwrap_or_default();

    macro_rules! hook_timeout_fallback {
        ($label:expr) => {{
            let elapsed_ms = hook_start.elapsed().as_millis();
            let msg = format!(
                "bash_tool: {} exceeded {}ms hook limit ({}ms elapsed); abandoning",
                $label, hook_timeout.as_millis(), elapsed_ms
            );
            tracing::debug!("{}", msg);
            crate::observability::log_message(
                &msg,
                "warning",
                Some(serde_json::json!({
                    "label": $label,
                    "elapsed_ms": elapsed_ms,
                    "hook_timeout_ms": hook_timeout.as_millis(),
                })),
            );
            signal_daemon_bash_session_end(BashSessionEndSignal {
                repo_work_dir: &repo_working_dir,
                original_cwd,
                session_id: context.session_id,
                tool_use_id: context.tool_use_id,
                agent_id: context.agent_id,
                metadata: &metadata,
                trace_id: context.trace_id,
                ended_at_ns,
                command: context.command,
            });
            return Ok(BashPostHookResult {
                action: BashCheckpointAction::HookTimeout,
            });
        }};
    }

    let pre_snapshot = query_daemon_bash_snapshot(context.session_id, context.tool_use_id);

    match pre_snapshot {
        Some(pre) => {
            if hook_start.elapsed() >= hook_timeout {
                hook_timeout_fallback!("post-hook before snapshot");
            }

            let post_wm: Option<DaemonWatermarks> =
                if pre.effective_worktree_wm.is_some() || !pre.per_file_wm.is_empty() {
                    Some(DaemonWatermarks {
                        per_file: pre.per_file_wm.clone(),
                        worktree: pre.effective_worktree_wm,
                    })
                } else {
                    None
                };
            let result = match snapshot(
                repo_root,
                context.session_id,
                context.tool_use_id,
                post_wm.as_ref(),
            ) {
                Ok(post) => {
                    let diff_result = diff(&pre, &post);

                    if diff_result.is_empty() {
                        tracing::debug!("Bash tool {}: no changes detected", invocation_key);
                        Ok(BashPostHookResult {
                            action: BashCheckpointAction::NoChanges,
                        })
                    } else {
                        let paths = diff_result.all_changed_paths();
                        tracing::debug!(
                            "Bash tool {}: {} files changed ({} created, {} modified)",
                            invocation_key,
                            paths.len(),
                            diff_result.created.len(),
                            diff_result.modified.len(),
                        );

                        Ok(BashPostHookResult {
                            action: BashCheckpointAction::Checkpoint(paths),
                        })
                    }
                }
                Err(e) => {
                    tracing::debug!("Post-snapshot failed: {}; returning SnapshotFailed", e);
                    Ok(BashPostHookResult {
                        action: BashCheckpointAction::SnapshotFailed,
                    })
                }
            };

            signal_daemon_bash_session_end(BashSessionEndSignal {
                repo_work_dir: &repo_working_dir,
                original_cwd,
                session_id: context.session_id,
                tool_use_id: context.tool_use_id,
                agent_id: context.agent_id,
                metadata: &metadata,
                trace_id: context.trace_id,
                ended_at_ns,
                command: context.command,
            });

            result
        }
        None => {
            tracing::debug!(
                "Pre-snapshot not found in daemon for {}; returning MissingPreSnapshot",
                invocation_key
            );
            signal_daemon_bash_session_end(BashSessionEndSignal {
                repo_work_dir: &repo_working_dir,
                original_cwd,
                session_id: context.session_id,
                tool_use_id: context.tool_use_id,
                agent_id: context.agent_id,
                metadata: &metadata,
                trace_id: context.trace_id,
                ended_at_ns,
                command: context.command,
            });
            Ok(BashPostHookResult {
                action: BashCheckpointAction::MissingPreSnapshot,
            })
        }
    }
}
