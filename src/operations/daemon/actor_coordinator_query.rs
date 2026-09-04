#[allow(unused_imports)]
use super::*;
use crate::error::GitAiError;
use crate::model::checkpoint_request::CheckpointRequest;
use crate::operations::daemon::git_backend::GitBackend;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::oneshot;

impl ActorDaemonCoordinator {
    pub(crate) async fn apply_trace_payload_to_state(
        self: &Arc<Self>,
        payload: Value,
    ) -> Result<TracePayloadApplyOutcome, GitAiError> {
        let payload_root_sid = Self::trace_payload_root_sid(&payload);
        let event = payload
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if event == TRACE_CONNECTION_CLOSED_EVENT {
            let Some(root_sid) = payload_root_sid.as_deref() else {
                return Ok(TracePayloadApplyOutcome::None);
            };
            {
                let mut normalizer = self.normalizer.lock().await;
                let _ = normalizer.sweep_orphans_for_roots(&[root_sid.to_string()]);
            }
            let replaced_family =
                self.replace_pending_root_entry(root_sid, FamilySequencerEntry::Canceled)?;
            let outcome = if replaced_family.is_some() {
                TracePayloadApplyOutcome::QueuedFamily
            } else {
                TracePayloadApplyOutcome::None
            };
            self.clear_trace_root_tracking(root_sid)?;
            self.drain_ready_family_sequencers_after_root_cleared(replaced_family)?;
            return Ok(outcome);
        }

        self.maybe_append_pending_root_from_trace_payload(&payload)?;
        let emitted = {
            let mut normalizer = self.normalizer.lock().await;
            normalizer.ingest_payload(&payload)?
        };
        let Some(command) = emitted else {
            if is_terminal_root_trace_event(
                &event,
                payload
                    .get("sid")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                payload_root_sid.as_deref().unwrap_or_default(),
            ) && let Some(root_sid) = payload_root_sid.as_deref()
                && let Some(family) =
                    self.replace_pending_root_entry(root_sid, FamilySequencerEntry::Canceled)?
            {
                self.clear_trace_root_tracking(root_sid)?;
                self.drain_ready_family_sequencers_after_root_cleared(Some(family))?;
                return Ok(TracePayloadApplyOutcome::QueuedFamily);
            }
            return Ok(TracePayloadApplyOutcome::None);
        };
        let root_sid = command.root_sid.clone();

        let mut family_to_drain_after_clear = None;
        let outcome = if let Some(family) = self.replace_pending_root_entry(
            &root_sid,
            FamilySequencerEntry::ReadyCommand(Box::new(command.clone())),
        )? {
            self.cache_commit_file_timestamp_snapshots_for_command(&command)?;
            family_to_drain_after_clear = Some(family);
            TracePayloadApplyOutcome::QueuedFamily
        } else if let Some(family) = command.family_key.as_ref().map(|family| family.0.clone()) {
            // Every family command is routed by its ordered drain. Routing a
            // later non-sequencer command inline could otherwise overtake an
            // earlier command waiting for side-effect capacity.
            self.cache_commit_file_timestamp_snapshots_for_command(&command)?;
            let started_at_ns = command.started_at_ns;
            self.append_family_sequencer_entry(
                &family,
                started_at_ns,
                FamilySequencerEntry::ReadyCommand(Box::new(command)),
            )?;
            family_to_drain_after_clear = Some(family);
            TracePayloadApplyOutcome::QueuedFamily
        } else {
            if let Err(error) = self.coordinator.route_command(command).await {
                let _ = self.clear_trace_root_tracking(&root_sid);
                return Err(error);
            }
            TracePayloadApplyOutcome::None
        };
        self.clear_trace_root_tracking(&root_sid)?;
        self.drain_ready_family_sequencers_after_root_cleared(family_to_drain_after_clear)?;
        Ok(outcome)
    }

    pub(crate) async fn ingest_trace_payload_fast(
        self: Arc<Self>,
        payload: Value,
    ) -> Result<(), GitAiError> {
        if !is_trace_payload(&payload) {
            return Ok(());
        }
        let _ = self.apply_trace_payload_to_state(payload).await?;

        Ok(())
    }

    async fn ingest_admitted_checkpoint_payload(
        &self,
        request: CheckpointRequest,
    ) -> Result<ControlResponse, GitAiError> {
        let repo_work_dir = request.files[0].repo_work_dir.clone();
        let family = self.backend.resolve_family(&repo_work_dir)?;

        let (respond_to, response) = oneshot::channel();
        self.append_checkpoint_to_family_sequencer(&family.0, request, Some(respond_to))
            .await?;
        response.await.map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "checkpoint response channel closed",
            )
        })??;
        Ok(ControlResponse::ok(None, None))
    }

    pub(crate) async fn ingest_checkpoint_delivery(
        &self,
        delivery: crate::model::checkpoint_delivery::CheckpointDelivery,
    ) -> Result<ControlResponse, GitAiError> {
        delivery
            .validate()
            .map_err(|error| GitAiError::PresetError(error.to_string()))?;
        // Stamp the envelope identity onto the request so the applied
        // checkpoint records it; at-least-once outbox replay of the same
        // delivery then deduplicates instead of applying twice.
        let mut request = delivery.request;
        request.delivery_id = Some(delivery.delivery_id);
        self.ingest_validated_checkpoint_control_payload(request)
            .await
    }

    pub(crate) async fn ingest_checkpoint_control_payload(
        &self,
        request: CheckpointRequest,
    ) -> Result<ControlResponse, GitAiError> {
        crate::model::checkpoint_delivery::validate_checkpoint_request_bounds(&request)
            .map_err(|error| GitAiError::PresetError(error.to_string()))?;
        self.ingest_validated_checkpoint_control_payload(request)
            .await
    }

    async fn ingest_validated_checkpoint_control_payload(
        &self,
        mut request: CheckpointRequest,
    ) -> Result<ControlResponse, GitAiError> {
        crate::operations::daemon::checkpoint_stream_authority::authorize_checkpoint_stream_source(
            &mut request,
        )?;
        if request.files.is_empty() {
            return Ok(ControlResponse::ok(None, None));
        }
        // Register before notification and the trace-ingest fence. Automatic
        // and graceful restarts must see every accepted checkpoint side effect.
        let admission = self.begin_checkpoint_admission()?;
        self.notify_stream_worker_checkpoint(&admission, &request);
        self.ingest_admitted_checkpoint_payload(request).await
    }
}
