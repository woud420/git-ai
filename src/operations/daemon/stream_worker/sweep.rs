use super::{CheckpointNotification, Priority, ProcessingTask, StreamWorker};
use crate::config;
use crate::model::authorship_log_serialization::generate_session_id;
use crate::model::repository::streams_db::StreamRecord;
use crate::model::stream_types::StreamError;
use crate::model::stream_watermark::{WatermarkStrategy, WatermarkType};
use crate::operations::streams::agent::{SHARED_STREAM_SESSION_ID, StreamDescriptor};
use crate::operations::streams::sweep::StreamFormat;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

impl StreamWorker {
    /// Run a sweep across all agents to discover new/behind sessions.
    pub(super) async fn run_sweep(&mut self, priority: Priority) -> Result<(), String> {
        use crate::operations::daemon::sweep_coordinator::SweepItem;

        let items = self
            .sweep_coordinator
            .run_sweep()
            .map_err(|e| e.to_string())?;

        tracing::info!(target: "git_ai::operations::daemon::stream_worker", discovered = items.len(), "sweep completed");

        for (i, item) in items.iter().enumerate().take(10) {
            match item {
                SweepItem::Session {
                    session_id,
                    tool,
                    canonical_path,
                    ..
                } => {
                    tracing::info!(target: "git_ai::operations::daemon::stream_worker",
                        index = i,
                        tool = %tool,
                        session_id = %session_id,
                        path = %canonical_path.display(),
                        "sweep item: session"
                    );
                }
                SweepItem::SharedStream {
                    tool,
                    stream_kind,
                    canonical_path,
                } => {
                    tracing::info!(target: "git_ai::operations::daemon::stream_worker",
                        index = i,
                        tool = %tool,
                        stream_kind = %stream_kind,
                        path = %canonical_path.display(),
                        "sweep item: shared stream"
                    );
                }
            }
        }
        if items.len() > 10 {
            tracing::info!(target: "git_ai::operations::daemon::stream_worker", remaining = items.len() - 10, "... and more sweep items");
        }

        let mut enqueued_this_sweep: HashSet<(PathBuf, String)> = HashSet::new();

        for item in items {
            match item {
                SweepItem::Session {
                    session_id,
                    tool,
                    canonical_path,
                    external_session_id,
                    external_parent_session_id,
                } => {
                    let inferred_cwd = crate::operations::streams::agent::get_agent(&tool)
                        .as_ref()
                        .and_then(|a| a.infer_cwd(&canonical_path));

                    let tasks = self.enqueue_streams_for_session(
                        &tool,
                        &canonical_path,
                        priority,
                        None,
                        None,
                        Some(external_session_id.as_str()),
                        external_parent_session_id.as_deref(),
                        inferred_cwd.as_deref(),
                        &session_id,
                        &mut enqueued_this_sweep,
                    );

                    for task in tasks {
                        self.priority_queue.push(task);
                    }
                }
                SweepItem::SharedStream {
                    tool,
                    stream_kind,
                    canonical_path,
                } => {
                    let dedup_key = (canonical_path.clone(), stream_kind.clone());
                    if self.in_flight.contains(&dedup_key)
                        || enqueued_this_sweep.contains(&dedup_key)
                    {
                        continue;
                    }

                    let Some(agent) = crate::operations::streams::agent::get_agent(&tool) else {
                        continue;
                    };
                    let Some(stream) = agent
                        .streams()
                        .into_iter()
                        .find(|s| s.stream_kind == stream_kind)
                    else {
                        continue;
                    };

                    if let Err(e) = self.ensure_stream_session(
                        SHARED_STREAM_SESSION_ID,
                        &tool,
                        &stream,
                        &canonical_path,
                        None,
                        None,
                        None,
                    ) {
                        tracing::warn!(target: "git_ai::operations::daemon::stream_worker",
                            tool = %tool,
                            stream_kind = %stream_kind,
                            error = %e,
                            "failed to ensure shared stream session, skipping"
                        );
                        continue;
                    }

                    enqueued_this_sweep.insert(dedup_key);
                    self.priority_queue.push(ProcessingTask {
                        priority,
                        session_id: SHARED_STREAM_SESSION_ID.to_string(),
                        stream_kind,
                        tool,
                        trace_id: None,
                        tool_use_id: None,
                        canonical_path,
                        repo_work_dir: None,
                        retry_count: 0,
                        next_retry_at: None,
                    });
                }
            }
        }

        Ok(())
    }

    /// Discover and enqueue subagent transcripts belonging to a main Claude session.
    ///
    /// Given a main session at `<project>/<uuid>.jsonl`, subagents live at
    /// `<project>/<uuid>/subagents/agent-*.jsonl`.
    pub(super) fn sweep_subagents_for_session(&mut self, notification: &CheckpointNotification) {
        let stream_path = &notification.stream_path;

        let subagents_dir = match stream_path.file_stem() {
            Some(stem) => stream_path.with_file_name(stem).join("subagents"),
            None => return,
        };

        if !subagents_dir.is_dir() {
            return;
        }

        let Ok(entries) = std::fs::read_dir(&subagents_dir) else {
            return;
        };

        let external_parent_session_id = stream_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        let lookback_cutoff = config::Config::get()
            .transcript_streaming_lookback_days()
            .map(|days| {
                std::time::SystemTime::now()
                    - std::time::Duration::from_secs(u64::from(days) * 24 * 60 * 60)
            });

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().map(|ext| ext == "jsonl") != Some(true) {
                continue;
            }

            let Some(external_session_id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
            else {
                continue;
            };

            let session_id = generate_session_id(&external_session_id, "claude");

            let canonical = crate::operations::git::canonicalize::canonicalize_or_self(&path);
            let dedup_key = (canonical.clone(), "transcript".to_string());
            if self.in_flight.contains(&dedup_key) {
                continue;
            }

            // Only apply lookback to NEW (untracked) subagent files — already-tracked
            // files are always processed so partial watermarks aren't abandoned.
            let path_str = canonical.display().to_string();
            let already_tracked = self
                .streams_db
                .get_stream(&session_id, "transcript", &path_str)
                .ok()
                .flatten()
                .is_some();

            if !already_tracked && let Some(cutoff) = lookback_cutoff {
                let too_old = path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .is_some_and(|mtime| mtime < cutoff);
                if too_old {
                    continue;
                }
            }

            // Ensure the subagent session exists in the DB
            if let Err(e) = self.ensure_subagent_session(
                &session_id,
                &canonical,
                &external_session_id,
                external_parent_session_id.as_deref(),
                notification.repo_work_dir.as_deref(),
            ) {
                tracing::warn!(target: "git_ai::operations::daemon::stream_worker",
                    session_id = %session_id,
                    error = %e,
                    "failed to ensure subagent session exists"
                );
                continue;
            }

            self.priority_queue.push(ProcessingTask {
                priority: Priority::Low,
                session_id,
                stream_kind: "transcript".to_string(),
                tool: "claude".to_string(),
                trace_id: Some(notification.trace_id.clone()),
                tool_use_id: None,
                canonical_path: canonical,
                repo_work_dir: notification.repo_work_dir.clone(),
                retry_count: 0,
                next_retry_at: None,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn enqueue_streams_for_session(
        &self,
        tool: &str,
        canonical_path: &Path,
        priority: Priority,
        trace_id: Option<String>,
        tool_use_id: Option<String>,
        external_session_id: Option<&str>,
        external_parent_session_id: Option<&str>,
        repo_work_dir: Option<&Path>,
        non_shared_session_id: &str,
        enqueued: &mut HashSet<(PathBuf, String)>,
    ) -> Vec<ProcessingTask> {
        let agent = crate::operations::streams::agent::get_agent(tool);
        let streams = agent.as_ref().map(|a| a.streams()).unwrap_or_default();
        let mut tasks = Vec::new();

        for stream in streams {
            // Shared streams are global singletons that serve all sessions. By
            // default they are processed during sweeps; Copilot OTEL is the
            // exception because a Copilot transcript checkpoint implies the
            // same underlying agent likely wrote fresh trace rows.
            if stream.shared
                && priority == Priority::Immediate
                && !Self::should_enqueue_shared_stream_immediately(tool, &stream)
            {
                continue;
            }

            let stream_path = match stream.resolve_path(canonical_path) {
                Some(p) if p.exists() => p,
                _ => continue,
            };

            let effective_session_id = if stream.shared {
                SHARED_STREAM_SESSION_ID.to_string()
            } else {
                non_shared_session_id.to_string()
            };

            if let Err(e) = self.ensure_stream_session(
                &effective_session_id,
                tool,
                &stream,
                &stream_path,
                external_session_id,
                external_parent_session_id,
                repo_work_dir,
            ) {
                tracing::warn!(target: "git_ai::operations::daemon::stream_worker",
                    session_id = %effective_session_id,
                    stream_kind = %stream.stream_kind,
                    error = %e,
                    "failed to ensure stream session exists"
                );
                continue;
            }

            let dedup_key = (stream_path.clone(), stream.stream_kind.to_string());
            if self.in_flight.contains(&dedup_key) || enqueued.contains(&dedup_key) {
                continue;
            }

            enqueued.insert(dedup_key);
            tasks.push(ProcessingTask {
                priority,
                session_id: effective_session_id,
                stream_kind: stream.stream_kind.to_string(),
                tool: tool.to_string(),
                trace_id: trace_id.clone(),
                tool_use_id: tool_use_id.clone(),
                canonical_path: stream_path,
                repo_work_dir: repo_work_dir.map(|p| p.to_path_buf()),
                retry_count: 0,
                next_retry_at: None,
            });
        }

        tasks
    }

    pub(super) fn should_enqueue_shared_stream_immediately(
        tool: &str,
        stream: &StreamDescriptor,
    ) -> bool {
        matches!(tool, "copilot" | "github-copilot") && stream.stream_kind == "otel_traces"
    }

    pub(super) fn ensure_subagent_session(
        &self,
        session_id: &str,
        path: &Path,
        external_session_id: &str,
        external_parent_session_id: Option<&str>,
        repo_work_dir: Option<&Path>,
    ) -> Result<(), StreamError> {
        let path_str = path.display().to_string();
        if self
            .streams_db
            .get_stream(session_id, "transcript", &path_str)?
            .is_some()
        {
            return Ok(());
        }

        use crate::model::stream_watermark::ByteOffsetWatermark;

        let initial_watermark = ByteOffsetWatermark::new(0);
        let record = StreamRecord {
            session_id: session_id.to_string(),
            stream_kind: "transcript".to_string(),
            tool: "claude".to_string(),
            stream_path: path_str,
            stream_format: StreamFormat::ClaudeJsonl,
            watermark_type: WatermarkType::ByteOffset,
            watermark_value: initial_watermark.serialize(),
            external_session_id: external_session_id.to_string(),
            external_parent_session_id: external_parent_session_id.map(|s| s.to_string()),
            first_seen_at: chrono::Utc::now().timestamp(),
            last_processed_at: 0,
            last_known_size: 0,
            last_modified: None,
            processing_errors: 0,
            last_error: None,
            repo_work_dir: repo_work_dir.map(|p| p.display().to_string()),
        };

        self.streams_db.insert_stream(&record)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn ensure_stream_session(
        &self,
        session_id: &str,
        tool: &str,
        stream: &StreamDescriptor,
        stream_path: &Path,
        external_session_id: Option<&str>,
        external_parent_session_id: Option<&str>,
        repo_work_dir: Option<&Path>,
    ) -> Result<(), StreamError> {
        let path_str = stream_path.display().to_string();
        if self
            .streams_db
            .get_stream(session_id, stream.stream_kind, &path_str)?
            .is_some()
        {
            return Ok(());
        }

        let effective_wm_type = stream.effective_watermark_type(stream_path);
        let initial_watermark = effective_wm_type.create_initial_watermark();

        // For shared streams, external_session_id/parent/repo_work_dir are meaningless
        // since the resource serves all sessions — use empty/None to avoid stale first-caller data
        let is_shared = session_id == SHARED_STREAM_SESSION_ID;
        let record = StreamRecord {
            session_id: session_id.to_string(),
            stream_kind: stream.stream_kind.to_string(),
            tool: tool.to_string(),
            stream_path: path_str,
            stream_format: stream.effective_format(stream_path),
            watermark_type: effective_wm_type,
            watermark_value: initial_watermark.serialize(),
            external_session_id: if is_shared {
                String::new()
            } else {
                external_session_id.unwrap_or("").to_string()
            },
            external_parent_session_id: if is_shared {
                None
            } else {
                external_parent_session_id.map(|s| s.to_string())
            },
            first_seen_at: chrono::Utc::now().timestamp(),
            last_processed_at: 0,
            last_known_size: 0,
            last_modified: None,
            processing_errors: 0,
            last_error: None,
            repo_work_dir: if is_shared {
                None
            } else {
                repo_work_dir.map(|p| p.display().to_string())
            },
        };

        self.streams_db.insert_stream(&record)
    }
}
