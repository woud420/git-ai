use super::super::{JournalError, invalid};
use super::types::{NativeAdmissionState, StoredAdmissionRecord};
use crate::model::jj_observation::{
    JJ_OBSERVATION_SCHEMA_VERSION, JjOperationEvidence, MAX_JJ_OBSERVATION_BATCH_BYTES, is_root,
    validate_ids, validate_profile, validate_source,
};

pub(super) const PACKET_DOMAIN: &str = "git-ai/jj/native-admission/packet/v1";
pub(super) const STATE_DOMAIN: &str = "git-ai/jj/native-admission/state/v1";
pub(super) const MAX_PACKET_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_STATE_BYTES: usize = 128 * 1024;

pub(super) fn scope(
    source: &str,
    profile: &str,
    receipt: &str,
    baseline: &str,
    baseline_generation: u64,
) -> Result<(), JournalError> {
    validate_source(source)?;
    validate_source(receipt)?;
    validate_source(baseline)?;
    validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, profile)?;
    if baseline_generation != 1 {
        return Err(invalid("native admission baseline generation invalid"));
    }
    Ok(())
}

pub(super) fn heads(ids: &[String]) -> Result<(), JournalError> {
    validate_ids(ids, 32)?;
    if ids.is_empty() || ids.iter().any(|id| is_root(id)) {
        return Err(invalid("native admission head set invalid"));
    }
    Ok(())
}

pub(super) fn canonical_heads(ids: &[String]) -> Result<(), JournalError> {
    heads(ids)?;
    if ids.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid("native admission head set is not canonical"));
    }
    Ok(())
}

pub(super) fn contents<'a>(
    expected: u64,
    expected_heads: &[String],
    captured_heads: &[String],
    operations: impl ExactSizeIterator<Item = &'a JjOperationEvidence>,
) -> Result<(), JournalError> {
    if expected >= i64::MAX as u64 {
        return Err(invalid("native admission expected generation invalid"));
    }
    heads(expected_heads)?;
    heads(captured_heads)?;
    if operations.len() > 256 {
        return Err(invalid("native admission operation count limit exceeded"));
    }
    let mut seen = std::collections::HashSet::with_capacity(operations.len());
    let mut bytes = 0usize;
    for record in operations {
        record.validate()?;
        validate_ids(&record.parent_ids, 32)?;
        if record.operation_bytes.is_empty() || record.view_bytes.is_empty() {
            return Err(invalid("native admission raw evidence is empty"));
        }
        if !seen.insert(&record.operation_id) {
            return Err(invalid("native admission duplicate operation identity"));
        }
        bytes = bytes
            .checked_add(record.operation_bytes.len())
            .and_then(|bytes| bytes.checked_add(record.view_bytes.len()))
            .filter(|bytes| *bytes <= MAX_JJ_OBSERVATION_BATCH_BYTES)
            .ok_or_else(|| invalid("native admission raw evidence byte limit exceeded"))?;
    }
    Ok(())
}

impl StoredAdmissionRecord {
    pub(super) fn validate(&self, source: &str, generation: u64) -> Result<(), JournalError> {
        if self.record_version != 1 || self.domain != PACKET_DOMAIN || self.source_id != source {
            return Err(invalid(
                "native admission packet identity or version invalid",
            ));
        }
        scope(
            &self.source_id,
            &self.reader_profile,
            &self.initialization_receipt_id,
            &self.baseline_id,
            self.baseline_generation,
        )?;
        contents(
            self.expected_admission_generation,
            &self.expected_admitted_head_ids,
            &self.captured_head_ids,
            self.operations.iter(),
        )?;
        if generation != self.expected_admission_generation + 1 {
            return Err(invalid("native admission packet generation mismatch"));
        }
        canonical_heads(&self.expected_admitted_head_ids)?;
        canonical_heads(&self.captured_head_ids)
    }
}

impl NativeAdmissionState {
    pub(super) fn validate(&self, source: &str, admission: &str) -> Result<(), JournalError> {
        if self.state_version != 1
            || self.domain != STATE_DOMAIN
            || self.source_id != source
            || self.admission_id != admission
            || self.generation == 0
            || self.generation > i64::MAX as u64
        {
            return Err(invalid(
                "native admission state identity or generation invalid",
            ));
        }
        validate_source(admission)?;
        scope(
            &self.source_id,
            &self.reader_profile,
            &self.initialization_receipt_id,
            &self.baseline_id,
            self.baseline_generation,
        )?;
        canonical_heads(&self.admitted_head_ids)
    }
}
