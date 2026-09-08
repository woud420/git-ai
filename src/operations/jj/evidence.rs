//! Joins captured operation/view bytes after native semantic-hash verification.
//! This verifies one envelope, not ancestry, admission, or attribution.

use super::operation::{DecodedJjOperation, JjDecodeError, decode_operation};
use super::view::{DecodedJjView, JjViewDecodeError, decode_view};
use crate::model::jj_observation::{
    JJ_OBSERVATION_SCHEMA_VERSION, JjObservationError, JjOperationEvidence, validate_profile,
};
use std::fmt;

pub struct VerifiedJjEvidence<'a> {
    evidence: &'a JjOperationEvidence,
    operation: DecodedJjOperation,
    view: DecodedJjView,
}

impl<'a> VerifiedJjEvidence<'a> {
    pub fn evidence(&self) -> &'a JjOperationEvidence {
        self.evidence
    }

    pub fn operation(&self) -> &DecodedJjOperation {
        &self.operation
    }

    pub fn view(&self) -> &DecodedJjView {
        &self.view
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JjEvidenceError {
    Profile(JjObservationError),
    Envelope(JjObservationError),
    Operation(JjDecodeError),
    View(JjViewDecodeError),
    OperationIdentityMismatch,
    ParentMismatch,
    ViewIdentityMismatch,
}

impl fmt::Display for JjEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(error) => write!(formatter, "jj evidence profile: {error}"),
            Self::Envelope(error) => write!(formatter, "jj evidence envelope: {error}"),
            Self::Operation(error) => fmt::Display::fmt(error, formatter),
            Self::View(error) => fmt::Display::fmt(error, formatter),
            Self::OperationIdentityMismatch => {
                formatter.write_str("jj evidence operation identity mismatch")
            }
            Self::ParentMismatch => formatter.write_str("jj evidence ordered parent mismatch"),
            Self::ViewIdentityMismatch => formatter.write_str("jj evidence view identity mismatch"),
        }
    }
}

impl std::error::Error for JjEvidenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Profile(error) | Self::Envelope(error) => Some(error),
            Self::Operation(error) => Some(error),
            Self::View(error) => Some(error),
            Self::OperationIdentityMismatch | Self::ParentMismatch | Self::ViewIdentityMismatch => {
                None
            }
        }
    }
}

/// Borrows the exact captured evidence, preserving its raw bytes and parent order.
/// Unknown ancestors and unrecorded predecessors are permitted at this layer.
pub fn verify_evidence<'a>(
    profile: &str,
    evidence: &'a JjOperationEvidence,
) -> Result<VerifiedJjEvidence<'a>, JjEvidenceError> {
    validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, profile).map_err(JjEvidenceError::Profile)?;
    evidence.validate().map_err(JjEvidenceError::Envelope)?;
    let operation = decode_operation(profile, &evidence.operation_id, &evidence.operation_bytes)
        .map_err(JjEvidenceError::Operation)?;
    if operation.operation_id != evidence.operation_id {
        return Err(JjEvidenceError::OperationIdentityMismatch);
    }
    if operation.parent_ids != evidence.parent_ids {
        return Err(JjEvidenceError::ParentMismatch);
    }
    if operation.view_id != evidence.view_id {
        return Err(JjEvidenceError::ViewIdentityMismatch);
    }
    let view = decode_view(profile, &evidence.view_id, &evidence.view_bytes)
        .map_err(JjEvidenceError::View)?;
    if view.view_id != operation.view_id {
        return Err(JjEvidenceError::ViewIdentityMismatch);
    }
    Ok(VerifiedJjEvidence {
        evidence,
        operation,
        view,
    })
}
