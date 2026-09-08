#[allow(unused_imports)]
use super::*;
use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use std::sync::atomic::Ordering;

impl ActorDaemonCoordinator {
    pub(crate) fn has_inflight_family_effects(&self) -> bool {
        self.inflight_effects_by_family
            .lock()
            .map(|map| !map.is_empty())
            .unwrap_or(true)
    }

    /// Work an automatic restart would abandon. Read the sequencer before the
    /// in-flight map: a drain registers its guard before popping, so this order
    /// observes one side or the other across that handoff.
    #[cfg(test)]
    pub(crate) fn has_pending_attribution_work(&self) -> bool {
        if self.pending_checkpoint_admissions.load(Ordering::Acquire) > 0
            || self.queued_trace_payloads.load(Ordering::Acquire) > 0
            || self.next_trace_ingest_seq.load(Ordering::Acquire)
                > self.processed_trace_ingest_seq.load(Ordering::Acquire)
        {
            return true;
        }

        match self.family_sequencers_by_family.lock() {
            Ok(map) => {
                if map.values().any(|state| {
                    state
                        .entries
                        .values()
                        .any(|entry| !matches!(entry, FamilySequencerEntry::PendingRoot))
                }) {
                    return true;
                }
            }
            Err(_) => return true,
        }

        self.has_inflight_family_effects()
    }

    pub(crate) fn begin_checkpoint_admission(
        &self,
    ) -> Result<CheckpointAdmissionGuard<'_>, GitAiError> {
        let _sequencers = self.family_sequencers_by_family.lock().map_err(|_| {
            PersistenceError::LockPoisoned {
                what: "family sequencer map",
            }
        })?;
        if !self.accepting_checkpoints.load(Ordering::Acquire) || self.is_shutting_down() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "daemon is shutting down",
            )
            .into());
        }
        self.pending_checkpoint_admissions
            .fetch_add(1, Ordering::AcqRel);
        Ok(CheckpointAdmissionGuard { coordinator: self })
    }

    /// Closes checkpoint admission and checks all attribution work already
    /// visible to the daemon before an automatic restart.
    pub(crate) fn try_request_idle_restart(&self, action: DaemonExitAction) -> bool {
        let sequencers = match self.family_sequencers_by_family.lock() {
            Ok(sequencers) => sequencers,
            Err(_) => return false,
        };
        // A graceful shutdown may already own the closed gate while it drains.
        // Never reopen admission on that path.
        if !self.accepting_checkpoints.load(Ordering::Acquire) {
            return false;
        }
        self.accepting_checkpoints.store(false, Ordering::Release);

        let has_queued_entries = sequencers.values().any(|state| {
            state
                .entries
                .values()
                .any(|entry| !matches!(entry, FamilySequencerEntry::PendingRoot))
        });
        let busy = self.pending_checkpoint_admissions.load(Ordering::Acquire) > 0
            || self.queued_trace_payloads.load(Ordering::Acquire) > 0
            || self.next_trace_ingest_seq.load(Ordering::Acquire)
                > self.processed_trace_ingest_seq.load(Ordering::Acquire)
            || has_queued_entries
            || self.has_inflight_family_effects();
        if busy {
            self.accepting_checkpoints.store(true, Ordering::Release);
            return false;
        }
        self.shutdown_action.store(action.as_u8(), Ordering::SeqCst);
        self.shutting_down.store(true, Ordering::Release);
        drop(sequencers);
        self.notify_shutdown_requested();
        true
    }

    pub(crate) fn set_checkpoint_acceptance(&self, accepting: bool) -> Result<bool, GitAiError> {
        // The sequencer lock makes closing admission atomic with checkpoint
        // insertion. Accepted entries are therefore all visible to the drain
        // before a graceful shutdown response can be sent.
        let _sequencers = self.family_sequencers_by_family.lock().map_err(|_| {
            PersistenceError::LockPoisoned {
                what: "family sequencer map",
            }
        })?;
        Ok(self.accepting_checkpoints.swap(accepting, Ordering::AcqRel))
    }

    fn has_actionable_family_entries(&self) -> Result<bool, GitAiError> {
        let sequencers = self.family_sequencers_by_family.lock().map_err(|_| {
            PersistenceError::LockPoisoned {
                what: "family sequencer map",
            }
        })?;
        for (family, state) in sequencers.iter() {
            let Some((order, entry)) = state.entries.first_key_value() else {
                continue;
            };
            if matches!(entry, FamilySequencerEntry::PendingRoot) {
                continue;
            }
            let entry_root_sid = match entry {
                FamilySequencerEntry::ReadyCommand(command) => Some(command.root_sid.as_str()),
                _ => None,
            };
            if !self.family_entry_blocked_by_prior_open_trace_root(
                family,
                order.started_at_ns,
                entry_root_sid,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) async fn drain_accepted_attribution_work(&self) -> Result<(), GitAiError> {
        loop {
            self.drain_all_ready_family_sequencers().await?;
            let trace_ingest_pending = self.queued_trace_payloads.load(Ordering::Acquire) > 0
                || self.next_trace_ingest_seq.load(Ordering::Acquire)
                    > self.processed_trace_ingest_seq.load(Ordering::Acquire);
            if !trace_ingest_pending
                && !self.has_actionable_family_entries()?
                && self.pending_checkpoint_admissions.load(Ordering::Acquire) == 0
                && !self.has_inflight_family_effects()
            {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }
}
