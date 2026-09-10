//! Durable observations of registered jj history, separate from attribution.

use crate::config::Config;
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::workspace_context::WorkspaceContext;
use std::time::Instant;

mod error;
mod types;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod verify;

pub use error::JjNativeAdmissionError;
pub use types::{
    DurableNativeAdmission, NativeAdmissionCursor, NativeAdmissionOutcome, NativeAdmissionReceipt,
    NativeReconciliationOutcome, RegisteredNativeAdmission, RegisteredNativeAdmissionState,
};

/// Untrusted optimistic constraints; a sampled cursor does not authorize a write.
pub struct NativeAdmissionExpectation<'a> {
    pub source_id: &'a str,
    pub initialization_receipt_id: &'a str,
    pub baseline_id: &'a str,
    pub generation: u64,
    pub admitted_head_ids: &'a [String],
}

/// Untrusted scope and progress constraints, including the remembered selected workspace.
pub struct NativeReconciliationExpectation<'a> {
    pub admission: NativeAdmissionExpectation<'a>,
    pub workspace_name: &'a str,
    pub attachment_id: &'a str,
}

/// Rechecks the current source and saved evidence without collecting ancestry.
pub fn read_registered_admission_state(
    journal: &JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<RegisteredNativeAdmissionState, JjNativeAdmissionError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::read_state(journal, context, config, deadline, read_budget)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (journal, context, config, deadline, read_budget);
        Err(JjNativeAdmissionError::UnsupportedPlatform)
    }
}

/// Collects against the original cutoff and commits only after retained-source
/// validation and complete transactional readback. This never enables attribution.
pub fn admit_registered_history(
    journal: &mut JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    expected: NativeAdmissionExpectation<'_>,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<NativeAdmissionOutcome, JjNativeAdmissionError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::admit_with_hooks(
            journal,
            context,
            config,
            expected,
            deadline,
            read_budget,
            &mut unix::Live,
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (journal, context, config, expected, deadline, read_budget);
        Err(JjNativeAdmissionError::UnsupportedPlatform)
    }
}

/// Reconciles one sampled head set without appending unchanged progress.
/// A changed set uses the original-cutoff admission and exact retry rules.
pub fn reconcile_registered_history(
    journal: &mut JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    expected: NativeReconciliationExpectation<'_>,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<NativeReconciliationOutcome, JjNativeAdmissionError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::reconcile_with_hooks(
            journal,
            context,
            config,
            expected,
            deadline,
            read_budget,
            &mut unix::Live,
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (journal, context, config, expected, deadline, read_budget);
        Err(JjNativeAdmissionError::UnsupportedPlatform)
    }
}

/// Verifies an exact historical receipt from saved bytes. This does not inspect
/// current paths or certify that the source still exists at its saved location.
pub fn read_native_admission(
    journal: &JjObservationJournal,
    source_id: &str,
    admission_id: &str,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<Option<DurableNativeAdmission>, JjNativeAdmissionError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::read_known(journal, source_id, admission_id, deadline, read_budget)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (journal, source_id, admission_id, deadline, read_budget);
        Err(JjNativeAdmissionError::UnsupportedPlatform)
    }
}
