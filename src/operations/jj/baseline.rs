//! Prepares an explicit current-state cutoff without validating older ancestry.

use super::evidence::{JjEvidenceError, VerifiedJjEvidence, verify_evidence};
use crate::model::jj_observation::{
    JJ_OBSERVATION_READER_PROFILE, JJ_OBSERVATION_SCHEMA_VERSION, JjObservationError,
    JjOperationEvidence, MAX_JJ_OBSERVATION_BATCH_BYTES, is_root, validate_ids, validate_profile,
};
use std::collections::HashSet;
use std::fmt;

pub const MAX_JJ_BASELINE_HEADS: usize = 32;
pub const MAX_JJ_BASELINE_RAW_BYTES: usize = MAX_JJ_OBSERVATION_BATCH_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JjBaselineBoundary {
    CurrentState,
}

/// Verified anchor bytes at a proposed cutoff, not an ancestry or admission proof.
/// Source binding, current integration, persistence and workspace readiness are
/// outside this pure preparation result.
pub struct PreparedCurrentStateBaseline<'a> {
    captured_head_ids: &'a [String],
    anchors: Vec<VerifiedJjEvidence<'a>>,
}

impl<'a> PreparedCurrentStateBaseline<'a> {
    pub fn reader_profile(&self) -> &'static str {
        JJ_OBSERVATION_READER_PROFILE
    }

    pub fn boundary(&self) -> JjBaselineBoundary {
        JjBaselineBoundary::CurrentState
    }

    pub fn captured_head_ids(&self) -> &'a [String] {
        self.captured_head_ids
    }

    /// Preserves evidence input order, which need not match the raw head order.
    pub fn anchors(&self) -> &[VerifiedJjEvidence<'a>] {
        &self.anchors
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JjBaselineError {
    Profile(JjObservationError),
    Heads(JjObservationError),
    Envelope(JjObservationError),
    Evidence(JjEvidenceError),
    Input(&'static str),
}

impl fmt::Display for JjBaselineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(error) => write!(formatter, "jj baseline profile: {error}"),
            Self::Heads(error) => write!(formatter, "jj baseline heads: {error}"),
            Self::Envelope(error) => write!(formatter, "jj baseline envelope: {error}"),
            Self::Evidence(error) => fmt::Display::fmt(error, formatter),
            Self::Input(message) => write!(formatter, "jj baseline {message}"),
        }
    }
}

impl std::error::Error for JjBaselineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Profile(error) | Self::Heads(error) | Self::Envelope(error) => Some(error),
            Self::Evidence(error) => Some(error),
            Self::Input(_) => None,
        }
    }
}

/// Borrows exact supplied head/anchor evidence after bounded native verification.
/// Anchor parents and predecessor metadata remain intact; older parent records
/// need not be supplied because this chooses a cutoff rather than closing a DAG.
pub fn prepare_current_state_baseline<'a>(
    profile: &str,
    captured_head_ids: &'a [String],
    anchors: &'a [JjOperationEvidence],
) -> Result<PreparedCurrentStateBaseline<'a>, JjBaselineError> {
    preflight(profile, captured_head_ids, anchors)?;
    let verified = anchors
        .iter()
        .map(|anchor| verify_evidence(profile, anchor).map_err(JjBaselineError::Evidence))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PreparedCurrentStateBaseline {
        captured_head_ids,
        anchors: verified,
    })
}

fn preflight(
    profile: &str,
    captured_head_ids: &[String],
    anchors: &[JjOperationEvidence],
) -> Result<(), JjBaselineError> {
    validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, profile).map_err(JjBaselineError::Profile)?;
    if captured_head_ids.is_empty() {
        return Err(JjBaselineError::Input("empty head set"));
    }
    if captured_head_ids.len() > MAX_JJ_BASELINE_HEADS {
        return Err(JjBaselineError::Input("head count limit exceeded"));
    }
    if anchors.len() > MAX_JJ_BASELINE_HEADS {
        return Err(JjBaselineError::Input("anchor count limit exceeded"));
    }
    validate_ids(captured_head_ids, MAX_JJ_BASELINE_HEADS).map_err(JjBaselineError::Heads)?;
    if captured_head_ids.iter().any(|head| is_root(head)) {
        return Err(JjBaselineError::Input("root cannot be a baseline head"));
    }
    if anchors.len() != captured_head_ids.len() {
        return Err(JjBaselineError::Input("anchor count does not match heads"));
    }

    let mut seen = HashSet::with_capacity(anchors.len());
    let mut raw_bytes = 0usize;
    for anchor in anchors {
        anchor.validate().map_err(JjBaselineError::Envelope)?;
        raw_bytes = raw_bytes
            .checked_add(anchor.operation_bytes.len())
            .and_then(|size| size.checked_add(anchor.view_bytes.len()))
            .filter(|size| *size <= MAX_JJ_BASELINE_RAW_BYTES)
            .ok_or(JjBaselineError::Input(
                "aggregate raw evidence byte limit exceeded",
            ))?;
        if !seen.insert(&anchor.operation_id) {
            return Err(JjBaselineError::Input("duplicate anchor identity"));
        }
        if !captured_head_ids.contains(&anchor.operation_id) {
            return Err(JjBaselineError::Input(
                "anchor identity is outside head set",
            ));
        }
    }
    Ok(())
}
