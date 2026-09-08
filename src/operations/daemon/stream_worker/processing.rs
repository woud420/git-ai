use super::{
    Priority, ProcessingTask, StreamWorker, extract_event_timestamp, transcript_collection_allowed,
};
use crate::metrics::{
    EventAttributes, MetricEvent, OtelTraceValues, PosEncoded, SessionEventValues,
};
use crate::model::authorship_log_serialization::{generate_session_id, generate_trace_id};
use crate::model::repository::streams_db::StreamsDatabase;
use crate::model::stream_types::StreamError;
use crate::operations::daemon::telemetry_worker::DaemonTelemetryWorkerHandle;
use crate::operations::daemon::transcript_redaction::redact_json_secrets;
use crate::operations::streams::agent::SHARED_STREAM_SESSION_ID;
use chrono::{TimeZone, Utc};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::time::Duration;

impl StreamWorker {
    /// Process the next task from the queue.
    pub(super) async fn process_next_task(&mut self) {
        // Move any now-ready delayed tasks back to the priority queue
        let now = std::time::Instant::now();
        self.promote_ready_delayed_tasks(now);

        let Some(task) = self.priority_queue.pop() else {
            return;
        };

        // Check if task is ready to be processed (retry delay)
        if let Some(next_retry_at) = task.next_retry_at
            && now < next_retry_at
        {
            self.delayed_tasks.push(task);
            return;
        }

        // Mark as in-flight
        self.in_flight
            .insert((task.canonical_path.clone(), task.stream_kind.clone()));

        // Process the session (spawn blocking to avoid blocking the worker loop)
        let db = self.streams_db.clone();
        let telemetry = self.telemetry_handle.clone();
        let shutdown_flag = self.shutdown_flag.clone();
        let task_clone = task.clone();

        let result = tokio::task::spawn_blocking(move || {
            Self::process_session_blocking(&db, &telemetry, &task_clone, &shutdown_flag)
        })
        .await;

        // Remove from in-flight
        self.in_flight
            .remove(&(task.canonical_path.clone(), task.stream_kind.clone()));

        // Handle result
        match result {
            Ok(Ok(())) => {
                // Success - task is done
            }
            Ok(Err(e)) => {
                // Error - handle retry logic
                self.handle_processing_error(task, e).await;
            }
            Err(e) => {
                // Panic in spawn_blocking
                tracing::error!(target: "git_ai::operations::daemon::stream_worker", error = %e, session_id = %task.session_id, "task panicked");
                self.handle_processing_error(
                    task,
                    StreamError::Fatal {
                        message: format!("task panicked: {}", e),
                    },
                )
                .await;
            }
        }
    }

    /// Process a session (blocking I/O).
    ///
    /// Loops over bounded batches from `read_incremental`, saving the watermark
    /// after each batch for crash resilience. Applies backpressure between
    /// batches when the telemetry buffer is above a threshold, sleeping to let
    /// the 3-second flush cycle drain it.
    pub(super) fn process_session_blocking(
        db: &StreamsDatabase,
        telemetry: &DaemonTelemetryWorkerHandle,
        task: &ProcessingTask,
        shutdown_flag: &AtomicBool,
    ) -> Result<(), StreamError> {
        let task_path_str = task.canonical_path.display().to_string();
        let stream = db
            .get_stream(&task.session_id, &task.stream_kind, &task_path_str)?
            .ok_or_else(|| StreamError::Fatal {
                message: format!("stream not found: {}", task.session_id),
            })?;

        let agent = crate::operations::streams::agent::get_agent(&task.tool).ok_or_else(|| {
            StreamError::Fatal {
                message: format!("unknown agent type: {}", task.tool),
            }
        })?;

        let mut current_watermark = stream.watermark_type.deserialize(&stream.watermark_value)?;
        let path = PathBuf::from(&stream.stream_path);
        let mut total_events = 0usize;
        let is_shared_stream = stream.session_id == SHARED_STREAM_SESSION_ID;

        // For shared streams, parent/repo/external attrs are meaningless since they'd
        // reflect whichever session first created the record. Per-event overrides handle
        // session_id and external_session_id; parent_session_id and repo_url are omitted.
        let parent_session_id = if is_shared_stream {
            None
        } else {
            stream
                .external_parent_session_id
                .as_ref()
                .map(|ext_pid| generate_session_id(ext_pid, &stream.tool))
        };

        // Resolve repo_work_dir with priority: task (hook) > DB > infer from transcript.
        // Shared streams serve multiple repos so repo_url must not be set at the batch level.
        let resolved_work_dir = if is_shared_stream {
            None
        } else {
            task.repo_work_dir
                .clone()
                .or_else(|| stream.repo_work_dir.as_ref().map(PathBuf::from))
                .or_else(|| agent.infer_cwd(&path))
        };

        // Collection is opt-in per repository: transcript content for sessions
        // outside allowed_repositories is never read or persisted. Shared
        // streams (multi-repo OTEL traces) have no batch-level repo and are
        // handled by their per-event session scoping instead.
        if !is_shared_stream && !transcript_collection_allowed(resolved_work_dir.as_deref()) {
            tracing::debug!(target: "git_ai::operations::daemon::stream_worker",
                session_id = %task.session_id,
                "skipping transcript processing: repository not in allowed_repositories"
            );
            return Ok(());
        }

        // Persist inferred cwd to DB if stream didn't already have one
        if !is_shared_stream
            && stream.repo_work_dir.is_none()
            && let Some(ref work_dir) = resolved_work_dir
        {
            let _ = db.update_repo_work_dir(
                &stream.session_id,
                &task.stream_kind,
                &stream.stream_path,
                &work_dir.display().to_string(),
            );
        }

        let mut base_attrs = EventAttributes::with_version(env!("CARGO_PKG_VERSION"))
            .session_id(stream.session_id.clone())
            .tool(&stream.tool);

        if !is_shared_stream {
            base_attrs = base_attrs
                .external_session_id(stream.external_session_id.clone())
                .external_parent_session_id_opt(stream.external_parent_session_id.clone())
                .parent_session_id_opt(parent_session_id);
        }

        if let Some(ref work_dir) = resolved_work_dir
            && let Some(url) = crate::repo_url::resolve_repo_url_from_path(work_dir)
        {
            base_attrs = base_attrs.repo_url(url);
        }

        let file_meta = std::fs::metadata(&path).ok();
        let is_initial_watermark = stream.watermark_value.is_empty()
            || stream.watermark_type.create_initial_watermark().serialize()
                == stream.watermark_value;
        let reader_session_id =
            agent.session_id_for_read(&stream.session_id, &stream.external_session_id);

        loop {
            if shutdown_flag.load(Ordering::Relaxed) {
                break;
            }

            let batch = agent.read_incremental(&path, current_watermark, reader_session_id)?;

            if batch.events.is_empty() {
                db.update_watermark(
                    &stream.session_id,
                    &task.stream_kind,
                    &stream.stream_path,
                    batch.new_watermark.as_ref(),
                )?;
                break;
            }

            let batch_count = batch.events.len();

            let is_otel_stream = task.stream_kind == "otel_traces";
            let metric_events: Vec<MetricEvent> = batch
                .events
                .into_iter()
                .enumerate()
                .filter_map(|(idx, raw_event)| {
                    let (eid, pid, tid) = agent.extract_event_ids(&raw_event);
                    let is_first_event = is_initial_watermark && total_events == 0 && idx == 0;
                    let event_ts = match &file_meta {
                        Some(meta) => {
                            agent.extract_event_timestamp(&raw_event, meta, is_first_event)
                        }
                        None => extract_event_timestamp(&raw_event)
                            .unwrap_or_else(|| crate::model::clock::now_secs() as u32),
                    };
                    let trace_id = generate_trace_id();
                    let mut event_attrs = base_attrs.clone().trace_id(trace_id);

                    if let Some(event_sid) = agent.extract_event_session_id(&raw_event) {
                        let derived_session_id = generate_session_id(&event_sid, &stream.tool);
                        event_attrs = event_attrs
                            .session_id(derived_session_id)
                            .external_session_id(event_sid);
                    } else if is_otel_stream {
                        tracing::debug!(target: "git_ai::operations::daemon::stream_worker",
                            session_id = %task.session_id,
                            "dropping OTEL span without extractable session identifier"
                        );
                        return None;
                    }

                    let attrs_sparse = event_attrs.to_sparse();
                    let raw_event = redact_json_secrets(raw_event);
                    Some(if is_otel_stream {
                        MetricEvent::from_values_with_timestamp(
                            OtelTraceValues::with_ids(raw_event, eid, pid, tid),
                            attrs_sparse,
                            Some(event_ts),
                        )
                    } else {
                        MetricEvent::from_values_with_timestamp(
                            SessionEventValues::with_ids(raw_event, eid, pid, tid),
                            attrs_sparse,
                            Some(event_ts),
                        )
                    })
                })
                .collect();

            if let Err(e) = telemetry.persist_metrics_blocking(&metric_events) {
                tracing::warn!(target: "git_ai::operations::daemon::stream_worker", %e, "telemetry: failed to persist transcript metrics locally");
            }

            // Backpressure: after synchronous local persistence, this mainly
            // throttles when metrics upload is available and pending DB rows
            // are accumulating faster than the flush loop can deliver them.
            // Short sleeps (~100ms) keep shutdown latency low since this runs
            // inside spawn_blocking. Capped at ~4s to avoid blocking forever.
            const BACKPRESSURE_THRESHOLD: usize = 5_000;
            const BACKPRESSURE_MAX_WAITS: usize = 40;
            for _ in 0..BACKPRESSURE_MAX_WAITS {
                if telemetry.metrics_buffer_len() < BACKPRESSURE_THRESHOLD {
                    break;
                }
                if shutdown_flag.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            total_events += batch_count;
            db.update_watermark(
                &stream.session_id,
                &task.stream_kind,
                &stream.stream_path,
                batch.new_watermark.as_ref(),
            )?;
            current_watermark = batch.new_watermark;
        }

        if let Ok(metadata) = std::fs::metadata(&stream.stream_path) {
            let file_size = metadata.len();
            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| Utc.timestamp_opt(d.as_secs() as i64, 0).unwrap());
            db.update_file_metadata(
                &stream.session_id,
                &task.stream_kind,
                &stream.stream_path,
                file_size,
                modified,
            )?;
        }

        tracing::debug!(target: "git_ai::operations::daemon::stream_worker",
            session_id = %task.session_id,
            events = total_events,
            "processed session"
        );

        Ok(())
    }

    /// Handle a processing error with exponential backoff.
    pub(super) async fn handle_processing_error(
        &mut self,
        task: ProcessingTask,
        error: StreamError,
    ) {
        match error {
            StreamError::Transient { message, .. } => {
                // Retry with exponential backoff: 5s, 30s, 5m, 30m
                let retry_count = task.retry_count + 1;
                let max_retries = 4;

                if retry_count >= max_retries {
                    tracing::error!(target: "git_ai::operations::daemon::stream_worker",
                        session_id = %task.session_id,
                        error = %message,
                        "max retries exceeded, dropping task"
                    );
                    if let Err(e) = self.streams_db.record_error(
                        &task.session_id,
                        &task.stream_kind,
                        &task.canonical_path.display().to_string(),
                        &format!("max retries: {}", message),
                    ) {
                        tracing::warn!(target: "git_ai::operations::daemon::stream_worker", session_id = %task.session_id, error = %e, "failed to record error in database");
                    }
                    return;
                }

                let delay = match retry_count {
                    1 => Duration::from_secs(5),
                    2 => Duration::from_secs(30),
                    3 => Duration::from_secs(5 * 60),
                    _ => Duration::from_secs(30 * 60),
                };

                tracing::warn!(target: "git_ai::operations::daemon::stream_worker",
                    session_id = %task.session_id,
                    error = %message,
                    retry = retry_count,
                    delay_secs = delay.as_secs(),
                    "transient error, will retry"
                );

                // Re-queue with updated retry count and next_retry_at
                let mut retried_task = task.clone();
                retried_task.retry_count = retry_count;
                retried_task.next_retry_at = Some(std::time::Instant::now() + delay);
                self.delayed_tasks.push(retried_task);
            }
            StreamError::Parse { line, message } => {
                // Parse errors are not retried
                tracing::error!(target: "git_ai::operations::daemon::stream_worker",
                    session_id = %task.session_id,
                    line = line,
                    error = %message,
                    "parse error, skipping session"
                );
                if let Err(e) = self.streams_db.record_error(
                    &task.session_id,
                    &task.stream_kind,
                    &task.canonical_path.display().to_string(),
                    &format!("parse line {}: {}", line, message),
                ) {
                    tracing::warn!(target: "git_ai::operations::daemon::stream_worker", session_id = %task.session_id, error = %e, "failed to record error in database");
                }
            }
            StreamError::Fatal { message } => {
                // Fatal errors are not retried
                tracing::error!(target: "git_ai::operations::daemon::stream_worker",
                    session_id = %task.session_id,
                    error = %message,
                    "fatal error, skipping session"
                );
                if let Err(e) = self.streams_db.record_error(
                    &task.session_id,
                    &task.stream_kind,
                    &task.canonical_path.display().to_string(),
                    &format!("fatal: {}", message),
                ) {
                    tracing::warn!(target: "git_ai::operations::daemon::stream_worker", session_id = %task.session_id, error = %e, "failed to record error in database");
                }
            }
        }
    }

    /// Drain immediate priority tasks before shutdown.
    pub(super) async fn drain_immediate_tasks(&mut self) {
        let immediate_tasks = self.take_immediate_tasks();

        tracing::info!(target: "git_ai::operations::daemon::stream_worker", tasks = immediate_tasks.len(), "draining immediate tasks");

        // Process immediate tasks
        for task in immediate_tasks {
            self.in_flight
                .insert((task.canonical_path.clone(), task.stream_kind.clone()));
            let db = self.streams_db.clone();
            let telemetry = self.telemetry_handle.clone();
            let shutdown_flag = self.shutdown_flag.clone();
            let task_clone = task.clone();

            let result = tokio::task::spawn_blocking(move || {
                Self::process_session_blocking(&db, &telemetry, &task_clone, &shutdown_flag)
            })
            .await;

            self.in_flight
                .remove(&(task.canonical_path.clone(), task.stream_kind.clone()));

            match result {
                Err(e) => {
                    tracing::error!(target: "git_ai::operations::daemon::stream_worker", error = %e, session_id = %task.session_id, "drain task panicked");
                }
                Ok(Err(e)) => {
                    tracing::error!(target: "git_ai::operations::daemon::stream_worker", error = %e, session_id = %task.session_id, "drain task processing error");
                }
                Ok(Ok(())) => {}
            }
        }
    }

    /// Removes immediate tasks while preserving lower-priority work.
    pub(super) fn take_immediate_tasks(&mut self) -> Vec<ProcessingTask> {
        let mut immediate_tasks = Vec::new();
        let mut retained_tasks = Vec::new();

        while let Some(task) = self.priority_queue.pop() {
            if task.priority == Priority::Immediate {
                immediate_tasks.push(task);
            } else {
                retained_tasks.push(task);
            }
        }
        self.priority_queue.extend(retained_tasks);

        let mut i = 0;
        while i < self.delayed_tasks.len() {
            if self.delayed_tasks[i].priority == Priority::Immediate {
                immediate_tasks.push(self.delayed_tasks.swap_remove(i));
            } else {
                i += 1;
            }
        }

        immediate_tasks
    }
}
