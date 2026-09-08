use super::{
    DaemonTelemetryWorkerHandle, ProcessingTask, StreamWorker, StreamsDatabase,
    extract_event_timestamp, transcript_collection_allowed,
};
use crate::metrics::{
    EventAttributes, MetricEvent, OtelTraceValues, PosEncoded, SessionEventValues,
};
use crate::model::authorship_log_serialization::generate_session_id;
use crate::model::authorship_log_serialization::generate_trace_id;
use crate::model::stream_types::StreamError;
use crate::operations::daemon::transcript_redaction::redact_json_secrets;
use crate::operations::streams::agent::SHARED_STREAM_SESSION_ID;
use chrono::{TimeZone, Utc};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

impl StreamWorker {
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
            tracing::debug!(
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
            // Release each redacted event tree before constructing the next one;
            // serialization must succeed for the whole batch before the DB write.
            let metric_event_jsons = batch
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
                        tracing::debug!(
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
                .map(|event| serde_json::to_string(&event))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| persistence_error(error.to_string()))?;

            telemetry
                .persist_metric_jsons_blocking(&metric_event_jsons)
                .map_err(|error| persistence_error(error.to_string()))?;

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

        tracing::debug!(
            session_id = %task.session_id,
            events = total_events,
            "processed session"
        );

        Ok(())
    }
}

fn persistence_error(message: String) -> StreamError {
    StreamError::Transient {
        message: format!("failed to persist transcript metrics locally: {message}"),
        retry_after: Duration::from_secs(5),
    }
}
