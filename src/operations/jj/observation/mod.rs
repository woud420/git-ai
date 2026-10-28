//! Bounded, resumable native admission. This observes evidence, never authorship.

use super::admission::{JjNativeAdmissionError, NativeAdmissionCursor, NativeAdmissionReceipt};
use super::ancestry::JjHeadClosure;
use crate::config::Config;
use crate::model::jj_observation::JjOperationEvidence;
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::workspace_context::WorkspaceContext;
use std::time::Instant;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod legacy;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod replay;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod writer;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::operations::jj) use legacy::read_complete;

/// Segment-local evidence is deliberately separate from the old complete-history
/// admission type. A linked segment does not repeat all of its dependencies.
#[derive(Debug)]
pub struct ObservedJjSegment {
    receipt: NativeAdmissionReceipt,
    local_operations: Vec<JjOperationEvidence>,
    head_closures: Vec<JjHeadClosure>,
}

impl ObservedJjSegment {
    pub fn receipt(&self) -> &NativeAdmissionReceipt {
        &self.receipt
    }
    pub fn original_baseline_id(&self) -> &str {
        self.receipt.baseline_id()
    }
    pub fn local_operations(&self) -> &[JjOperationEvidence] {
        &self.local_operations
    }
    pub fn head_closures(&self) -> &[JjHeadClosure] {
        &self.head_closures
    }
}

#[derive(Debug)]
pub enum JjObservationStep {
    Replaying,
    Unchanged,
    Admitted(Box<ObservedJjSegment>),
}

/// Owns one existing journal connection for the replay/cache lifetime. Opening a
/// new session starts without admission authority. No proof flag is persisted.
pub struct JjObservationSession {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    inner: replay::Session,
}

impl JjObservationSession {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn saved_cursor(&self) -> &NativeAdmissionCursor {
        &self.inner.expected
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn registration(&self) -> &super::registration::RegisteredJjCurrentState {
        &self.inner.registered
    }
    pub fn new(
        journal: JjObservationJournal,
        context: WorkspaceContext,
        config: &Config,
        deadline: Instant,
        reads: &mut ReadBudget,
    ) -> Result<Self, JjNativeAdmissionError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Ok(Self {
                inner: replay::Session::new(journal, context, config, deadline, reads)?,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (journal, context, config, deadline, reads);
            Err(JjNativeAdmissionError::UnsupportedPlatform)
        }
    }

    pub fn verified_cursor(&self) -> Option<&NativeAdmissionCursor> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.verified_cursor()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            None
        }
    }

    /// Reverify stored dependencies without admitting current native operations.
    pub fn replay_step(
        &mut self,
        config: &Config,
        deadline: Instant,
        reads: &mut ReadBudget,
    ) -> Result<JjObservationStep, JjNativeAdmissionError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.step_with_mode(config, deadline, reads, false)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (config, deadline, reads);
            Err(JjNativeAdmissionError::UnsupportedPlatform)
        }
    }

    /// Read segment-local evidence only after replay has verified the exact saved
    /// frontier. None means no verified saved segment is available yet.
    pub fn read_latest_segment(
        &mut self,
        config: &Config,
        deadline: Instant,
        reads: &mut ReadBudget,
    ) -> Result<Option<ObservedJjSegment>, JjNativeAdmissionError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.read_latest_segment(config, deadline, reads)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (config, deadline, reads);
            Err(JjNativeAdmissionError::UnsupportedPlatform)
        }
    }

    /// Replays at most one saved dependency per call, then yields without a read
    /// transaction. A ready session can admit one bounded segment per call.
    pub fn step(
        &mut self,
        config: &Config,
        deadline: Instant,
        reads: &mut ReadBudget,
    ) -> Result<JjObservationStep, JjNativeAdmissionError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.step(config, deadline, reads)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (config, deadline, reads);
            Err(JjNativeAdmissionError::UnsupportedPlatform)
        }
    }
}
