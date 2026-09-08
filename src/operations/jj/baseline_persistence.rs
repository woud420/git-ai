//! Persists and re-verifies a caller-selected cutoff in a source namespace.
//! This does not establish physical source continuity or admit later operations.

use super::baseline::{
    JjBaselineBoundary, JjBaselineError, PreparedCurrentStateBaseline,
    prepare_current_state_baseline,
};
use crate::model::jj_observation::JjOperationEvidence;
use crate::model::repository::jj_observation_journal::native_baseline::{
    NativeBaselineState, NativeInstallOutcome,
};
use crate::model::repository::jj_observation_journal::{
    JjObservationJournal, JournalError, ReadBudget,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineReceipt {
    state: NativeBaselineState,
}

impl BaselineReceipt {
    pub fn source_id(&self) -> &str {
        &self.state.source_id
    }
    pub fn reader_profile(&self) -> &str {
        &self.state.reader_profile
    }
    pub fn boundary(&self) -> JjBaselineBoundary {
        JjBaselineBoundary::CurrentState
    }
    pub fn baseline_id(&self) -> &str {
        &self.state.baseline_id
    }
    pub fn expected_native_generation(&self) -> u64 {
        0
    }
    pub fn generation(&self) -> u64 {
        self.state.generation
    }
    pub fn captured_head_ids(&self) -> &[String] {
        &self.state.captured_head_ids
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselinePersistenceOutcome {
    Installed(BaselineReceipt),
    AlreadyInstalled(BaselineReceipt),
}

/// An immutable, native-verified snapshot of the persisted cutoff record.
/// It proves neither current filesystem heads nor ancestry through its anchors.
pub struct DurableCurrentStateBaseline {
    receipt: BaselineReceipt,
    anchors: Vec<JjOperationEvidence>,
}

impl DurableCurrentStateBaseline {
    pub fn receipt(&self) -> &BaselineReceipt {
        &self.receipt
    }
    pub fn anchors(&self) -> &[JjOperationEvidence] {
        &self.anchors
    }
}

#[derive(Debug)]
pub enum JjBaselinePersistenceError {
    Baseline(JjBaselineError),
    Journal(JournalError),
}

impl fmt::Display for JjBaselinePersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Baseline(error) => write!(formatter, "jj baseline persistence: {error}"),
            Self::Journal(error) => write!(formatter, "jj baseline persistence: {error}"),
        }
    }
}

impl std::error::Error for JjBaselinePersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Baseline(error) => Some(error),
            Self::Journal(error) => Some(error),
        }
    }
}

/// Installs the first native epoch without touching opaque observation progress.
/// Source identity and current-head sampling remain the caller's responsibility.
pub fn persist_current_state_baseline(
    journal: &mut JjObservationJournal,
    source_id: &str,
    expected_native_generation: u64,
    prepared: &PreparedCurrentStateBaseline<'_>,
) -> Result<BaselinePersistenceOutcome, JjBaselinePersistenceError> {
    let anchors: Vec<_> = prepared
        .anchors()
        .iter()
        .map(|anchor| anchor.evidence())
        .collect();
    let outcome = journal
        .persist_native_baseline(
            source_id,
            expected_native_generation,
            prepared.reader_profile(),
            prepared.captured_head_ids(),
            &anchors,
        )
        .map_err(JjBaselinePersistenceError::Journal)?;
    Ok(match outcome {
        NativeInstallOutcome::Installed(state) => {
            BaselinePersistenceOutcome::Installed(BaselineReceipt { state })
        }
        NativeInstallOutcome::AlreadyInstalled(state) => {
            BaselinePersistenceOutcome::AlreadyInstalled(BaselineReceipt { state })
        }
    })
}

/// Repeats native hash/envelope validation after one bounded SQLite read snapshot.
/// Failure retains the caller's charges for every encoded BLOB already selected.
pub fn reopen_current_state_baseline(
    journal: &JjObservationJournal,
    source_id: &str,
    budget: &mut ReadBudget,
) -> Result<Option<DurableCurrentStateBaseline>, JjBaselinePersistenceError> {
    let Some(snapshot) = journal
        .read_native_baseline(source_id, budget)
        .map_err(JjBaselinePersistenceError::Journal)?
    else {
        return Ok(None);
    };
    prepare_current_state_baseline(
        &snapshot.record.reader_profile,
        &snapshot.record.captured_head_ids,
        &snapshot.record.anchors,
    )
    .map_err(JjBaselinePersistenceError::Baseline)?;
    Ok(Some(DurableCurrentStateBaseline {
        receipt: BaselineReceipt {
            state: snapshot.state,
        },
        anchors: snapshot.record.anchors,
    }))
}
