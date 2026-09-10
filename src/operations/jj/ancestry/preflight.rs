use super::{BaselineReceipt, JjAncestryError as E, JjAncestryInput};
use crate::model::jj_observation::{
    MAX_JJ_OBSERVATION_BATCH_BYTES, MAX_JJ_OBSERVATION_OPERATIONS, is_root, validate_ids,
};
use crate::operations::jj::baseline::MAX_JJ_BASELINE_HEADS;
use crate::operations::jj::operation::MAX_OPERATION_PARENTS;
use std::collections::HashSet;

pub(super) fn validate(receipt: &BaselineReceipt, input: &JjAncestryInput<'_>) -> Result<(), E> {
    if input.source_id != receipt.source_id() {
        return Err(E::Input("source identity mismatch"));
    }
    if input.reader_profile != receipt.reader_profile() {
        return Err(E::Input("reader profile mismatch"));
    }
    if input.baseline_id != receipt.baseline_id() {
        return Err(E::Input("baseline identity mismatch"));
    }
    if input.expected_native_generation != receipt.generation() {
        return Err(E::Input("native generation mismatch"));
    }
    if input.head_ids.is_empty() {
        return Err(E::Input("empty head set"));
    }
    if input.head_ids.len() > MAX_JJ_BASELINE_HEADS {
        return Err(E::Input("head count limit exceeded"));
    }
    if input.operations.len() > MAX_JJ_OBSERVATION_OPERATIONS {
        return Err(E::Input("operation count limit exceeded"));
    }
    validate_ids(input.head_ids, MAX_JJ_BASELINE_HEADS).map_err(E::Heads)?;
    if input.head_ids.iter().any(|id| is_root(id)) {
        return Err(E::Input("root cannot be an ancestry head"));
    }

    let mut seen = HashSet::with_capacity(input.operations.len());
    let mut raw_bytes = 0usize;
    for evidence in input.operations {
        if evidence.parent_ids.len() > MAX_OPERATION_PARENTS {
            return Err(E::Input("parent count limit exceeded"));
        }
        evidence.validate().map_err(E::Envelope)?;
        if !seen.insert(&evidence.operation_id) {
            return Err(E::Input("duplicate operation identity"));
        }
        if receipt.captured_head_ids().contains(&evidence.operation_id) {
            return Err(E::Input("resupplied boundary evidence"));
        }
        raw_bytes = raw_bytes
            .checked_add(evidence.operation_bytes.len())
            .and_then(|bytes| bytes.checked_add(evidence.view_bytes.len()))
            .filter(|bytes| *bytes <= MAX_JJ_OBSERVATION_BATCH_BYTES)
            .ok_or(E::Input("aggregate raw evidence byte limit exceeded"))?;
    }
    Ok(())
}
