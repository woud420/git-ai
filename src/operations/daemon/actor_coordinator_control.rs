#[allow(unused_imports)]
use super::*;
use crate::error::GitAiError;
use crate::operations::daemon::git_backend::GitBackend;
use std::path::Path;
use std::sync::atomic::Ordering;

impl ActorDaemonCoordinator {
    pub(crate) async fn watermarks_for_family(
        &self,
        repo_working_dir: String,
    ) -> Result<crate::model::domain::WatermarkState, GitAiError> {
        self.coordinator
            .watermarks_family(Path::new(&repo_working_dir))
            .await
    }

    pub(crate) async fn status_for_family(
        &self,
        repo_working_dir: String,
    ) -> Result<FamilyStatus, GitAiError> {
        let family = self.backend.resolve_family(Path::new(&repo_working_dir))?;
        let status = self
            .coordinator
            .status_family(Path::new(&repo_working_dir))
            .await?;
        let latest_seq = status.applied_seq;
        let family_key = family.0;
        Ok(FamilyStatus {
            family_key: family_key.clone(),
            latest_seq,
            last_error: status
                .last_error
                .or_else(|| self.latest_side_effect_error(&family_key).ok().flatten()),
        })
    }

    pub(crate) async fn sync_family(
        &self,
        repo_working_dir: String,
    ) -> Result<FamilyStatus, GitAiError> {
        let family = self.backend.resolve_family(Path::new(&repo_working_dir))?;
        // Family-scoped fence: a sync request only has to wait for trace roots
        // that could affect its own repository family. `await_completion` and
        // shutdown keep the global fence.
        self.wait_for_trace_ingest_processed_through_family(&family.0)
            .await;

        loop {
            // A source-family command can apply notes to a destination family,
            // so preserve the pre-detachment global completion fence.
            self.drain_all_ready_family_sequencers().await?;
            if !self.has_inflight_family_effects() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }

        self.status_for_family(repo_working_dir).await
    }

    /// Wait for the daemon to finish all in-flight work and telemetry flushing.
    ///
    /// Progress is logged every few seconds. Returns an `AwaitResult` describing
    /// whether the daemon was idle before the timeout and how much telemetry
    /// (if any) is still pending.
    pub(crate) async fn await_completion(&self, timeout_secs: u64) -> AwaitResult {
        use tokio::time::{Duration, Instant, timeout};

        let start = Instant::now();
        let deadline = start + Duration::from_secs(timeout_secs);
        let log_interval = Duration::from_secs(3);
        let mut last_log = start;

        let mut result = AwaitResult {
            done: false,
            timed_out: false,
            metrics_remaining: 0,
            notes_remaining: 0,
        };

        let mut maybe_log = |phase: &str| {
            let now = Instant::now();
            if now - last_log >= log_interval {
                tracing::info!(phase, "await: still waiting");
                eprintln!("await: still waiting for {}...", phase);
                last_log = now;
            }
        };

        // Phase 1: wait for the trace-ingest and family-sequencer work side.
        while !self.is_shutting_down() {
            let now = Instant::now();
            if now >= deadline {
                result.timed_out = true;
                break;
            }
            let remaining = deadline - now;

            maybe_log("daemon work");
            if timeout(remaining, self.wait_for_trace_ingest_processed_through())
                .await
                .is_err()
            {
                result.timed_out = true;
                break;
            }

            if self.is_shutting_down() {
                break;
            }

            let now = Instant::now();
            if now >= deadline {
                result.timed_out = true;
                break;
            }
            let remaining = deadline - now;

            if timeout(remaining, self.drain_all_ready_family_sequencers())
                .await
                .is_err()
            {
                result.timed_out = true;
                break;
            }

            if !self.has_pending_daemon_work() {
                break;
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        if self.is_shutting_down() {
            result.timed_out = true;
        }

        // Phase 2: drain the transcript/stream worker.
        if !result.timed_out
            && let Some(worker) = &self.stream_worker
        {
            let now = Instant::now();
            if now < deadline {
                let remaining = deadline - now;
                maybe_log("transcript processing");
                match timeout(remaining, worker.drain()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        tracing::warn!(error = %e, "await: transcript drain failed");
                    }
                    Err(_) => {
                        result.timed_out = true;
                    }
                }
            } else {
                result.timed_out = true;
            }
        }

        // Phase 3: flush telemetry and wait for the worker to finish.
        if !result.timed_out
            && let Some(worker) = &self.telemetry_worker
        {
            let now = Instant::now();
            if now < deadline {
                let remaining = deadline - now;
                maybe_log("telemetry flush");
                match timeout(remaining, worker.flush_and_wait()).await {
                    Ok(Ok(status)) => {
                        result.metrics_remaining = status.metrics_remaining;
                        result.notes_remaining = status.notes_remaining;
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(error = %e, "await: telemetry flush failed");
                    }
                    Err(_) => {
                        result.timed_out = true;
                    }
                }
            } else {
                result.timed_out = true;
            }
        }

        result.done = !result.timed_out
            && result.metrics_remaining == 0
            && result.notes_remaining == 0
            && !self.has_pending_daemon_work();
        result
    }

    pub(crate) fn has_pending_daemon_work(&self) -> bool {
        if self.pending_checkpoint_admissions.load(Ordering::Acquire) > 0
            || self.queued_trace_payloads.load(Ordering::Acquire) > 0
        {
            return true;
        }
        if self.next_trace_ingest_seq.load(Ordering::Acquire)
            > self.processed_trace_ingest_seq.load(Ordering::Acquire)
        {
            return true;
        }
        if self.has_open_trace_roots_that_may_mutate_refs() {
            return true;
        }
        if let Ok(map) = self.family_sequencers_by_family.lock() {
            for state in map.values() {
                if !state.entries.is_empty() {
                    return true;
                }
            }
        }
        self.has_inflight_family_effects()
    }

    pub(crate) fn notify_stream_worker_checkpoint(
        &self,
        _admission: &CheckpointAdmissionGuard<'_>,
        request: &crate::model::checkpoint_request::CheckpointRequest,
    ) {
        let Some(worker) = &self.stream_worker else {
            return;
        };
        let Some(stream_source) = &request.stream_source else {
            return;
        };
        let tool = request
            .agent_id
            .as_ref()
            .map(|agent_id| agent_id.tool.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let repo_work_dir = request.files.first().map(|file| file.repo_work_dir.clone());

        worker.notify_checkpoint(
            stream_source.session_id.clone(),
            tool,
            request.trace_id.clone(),
            request.metadata.get("tool_use_id").cloned(),
            stream_source.path.clone(),
            stream_source.format,
            repo_work_dir,
            stream_source.external_session_id.clone(),
            stream_source.external_parent_session_id.clone(),
        );
    }
}
