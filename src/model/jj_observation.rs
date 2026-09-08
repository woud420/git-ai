//! Captured jj evidence is opaque until a qualified native reader verifies it.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

pub const JJ_OBSERVATION_SCHEMA_VERSION: u16 = 1;
pub const JJ_OBSERVATION_READER_PROFILE: &str = "jj-simple-op-store/0.45.1";
pub const MAX_JJ_OBSERVATION_BATCH_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_JJ_OBSERVATION_OPERATION_BYTES: usize = 1024 * 1024;
pub const MAX_JJ_OBSERVATION_OPERATIONS: usize = 256;
pub const MAX_JJ_OBSERVATION_HEADS: usize = 128;
pub(crate) const MAX_JJ_OBSERVATION_PARENTS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JjOperationEvidence {
    pub operation_id: String,
    pub parent_ids: Vec<String>,
    pub view_id: String,
    pub operation_bytes: Vec<u8>,
    pub view_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JjObservationBatch {
    pub schema_version: u16,
    pub reader_profile: String,
    pub source_id: String,
    pub expected_generation: u64,
    pub expected_observed_heads: Vec<String>,
    pub captured_integrated_heads: Vec<String>,
    pub operations: Vec<JjOperationEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjObservationError(pub(crate) &'static str);

impl fmt::Display for JjObservationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "jj observation {}", self.0)
    }
}

impl std::error::Error for JjObservationError {}

pub(crate) fn validate_profile(version: u16, profile: &str) -> Result<(), JjObservationError> {
    if version != JJ_OBSERVATION_SCHEMA_VERSION {
        return Err(JjObservationError("unsupported schema version"));
    }
    if profile != JJ_OBSERVATION_READER_PROFILE {
        return Err(JjObservationError("unsupported reader profile"));
    }
    Ok(())
}

pub(crate) fn validate_source(source: &str) -> Result<(), JjObservationError> {
    if !is_lower_hex(source, 64) {
        return Err(JjObservationError("invalid source identity"));
    }
    Ok(())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn is_root(operation_id: &str) -> bool {
    operation_id.len() == 128 && operation_id.bytes().all(|byte| byte == b'0')
}

pub(crate) fn validate_operation_id(value: &str) -> Result<(), JjObservationError> {
    if !is_lower_hex(value, 128) {
        return Err(JjObservationError("invalid operation or view identity"));
    }
    Ok(())
}

pub(crate) fn validate_ids(ids: &[String], limit: usize) -> Result<(), JjObservationError> {
    if ids.len() > limit {
        return Err(JjObservationError("identity count limit exceeded"));
    }
    let mut seen = HashSet::with_capacity(ids.len());
    for id in ids {
        validate_operation_id(id)?;
        if !seen.insert(id) {
            return Err(JjObservationError("duplicate identity"));
        }
    }
    Ok(())
}

impl JjOperationEvidence {
    pub(crate) fn validate(&self) -> Result<(), JjObservationError> {
        validate_operation_id(&self.operation_id)?;
        validate_operation_id(&self.view_id)?;
        if is_root(&self.operation_id) {
            return Err(JjObservationError(
                "root sentinel cannot be stored as an operation",
            ));
        }
        if self.parent_ids.is_empty() {
            return Err(JjObservationError("operation parent gap"));
        }
        validate_ids(&self.parent_ids, MAX_JJ_OBSERVATION_PARENTS)?;
        if self.operation_bytes.len() > MAX_JJ_OBSERVATION_OPERATION_BYTES
            || self.view_bytes.len()
                > MAX_JJ_OBSERVATION_OPERATION_BYTES.saturating_sub(self.operation_bytes.len())
        {
            return Err(JjObservationError("operation evidence byte limit exceeded"));
        }
        Ok(())
    }
}

impl JjObservationBatch {
    pub(crate) fn validate(&self) -> Result<(), JjObservationError> {
        validate_profile(self.schema_version, &self.reader_profile)?;
        validate_source(&self.source_id)?;
        validate_ids(&self.expected_observed_heads, MAX_JJ_OBSERVATION_HEADS)?;
        validate_ids(&self.captured_integrated_heads, MAX_JJ_OBSERVATION_HEADS)?;
        if self.captured_integrated_heads.is_empty() {
            return Err(JjObservationError("captured head gap"));
        }
        if self.operations.len() > MAX_JJ_OBSERVATION_OPERATIONS {
            return Err(JjObservationError("operation count limit exceeded"));
        }
        let mut seen = HashSet::with_capacity(self.operations.len());
        let mut raw_bytes = 0usize;
        for operation in &self.operations {
            operation.validate()?;
            raw_bytes = raw_bytes
                .checked_add(operation.operation_bytes.len())
                .and_then(|bytes| bytes.checked_add(operation.view_bytes.len()))
                .filter(|bytes| *bytes <= MAX_JJ_OBSERVATION_BATCH_BYTES)
                .ok_or(JjObservationError("batch evidence byte limit exceeded"))?;
            if !seen.insert(&operation.operation_id) {
                return Err(JjObservationError("duplicate operation identity"));
            }
        }
        Ok(())
    }
}
