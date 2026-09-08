//! Read-only collection of registered jj history to its original cutoff or root.
//! Returned evidence describes sampled history, not current-at-return state or
//! durable admission. Collection never advances a journal cursor or attribution.

use super::capture::{CapturedJjHistoryEvidence, JjCaptureError};
use super::registration::{JjRegistrationError, RegisteredJjCurrentState};
use crate::config::Config;
use crate::model::jj_observation::JjOperationEvidence;
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::workspace_context::WorkspaceContext;
use std::error::Error;
use std::fmt;
use std::time::Instant;

/// Owned, complete structural closure of the sampled heads to the saved cutoff
/// or native root. This value cannot authorize later admission or attribution.
pub struct CollectedJjHistory {
    registration: RegisteredJjCurrentState,
    evidence: CapturedJjHistoryEvidence,
}

impl CollectedJjHistory {
    pub fn registration(&self) -> &RegisteredJjCurrentState {
        &self.registration
    }

    pub fn head_ids(&self) -> &[String] {
        self.evidence.head_ids()
    }

    pub fn ordered_operations(&self) -> &[JjOperationEvidence] {
        self.evidence.ordered_operations()
    }

    pub fn reached_baseline_ids(&self) -> &[String] {
        self.evidence.reached_baseline_ids()
    }

    pub fn reaches_root(&self) -> bool {
        self.evidence.reaches_root()
    }
}

#[derive(Debug)]
pub enum JjHistoryCollectionError {
    Registration(JjRegistrationError),
    Capture(JjCaptureError),
    UnsupportedPlatform,
}

impl fmt::Display for JjHistoryCollectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("jj history collection: ")?;
        match self {
            Self::Registration(error) => fmt::Display::fmt(error, formatter),
            Self::Capture(error) => fmt::Display::fmt(error, formatter),
            Self::UnsupportedPlatform => formatter.write_str("unsupported native history platform"),
        }
    }
}

impl Error for JjHistoryCollectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Registration(error) => Some(error),
            Self::Capture(error) => Some(error),
            Self::UnsupportedPlatform => None,
        }
    }
}

impl From<JjRegistrationError> for JjHistoryCollectionError {
    fn from(error: JjRegistrationError) -> Self {
        Self::Registration(error)
    }
}

impl From<JjCaptureError> for JjHistoryCollectionError {
    fn from(error: JjCaptureError) -> Self {
        Self::Capture(error)
    }
}

/// Samples and verifies one registered source under fixed limits. Missing or
/// incomplete evidence rejects the whole collection without changing storage.
pub fn collect_registered_history(
    journal: &JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<CollectedJjHistory, JjHistoryCollectionError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let (registration, evidence) =
            super::registration::collect_history(journal, context, config, deadline, read_budget)?;
        Ok(CollectedJjHistory {
            registration,
            evidence,
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (journal, context, config, deadline, read_budget);
        Err(JjHistoryCollectionError::UnsupportedPlatform)
    }
}
