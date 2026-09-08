use super::directories::DirectoryRegistry;
use super::source::BoundSource;
use super::{CaptureBudget, CaptureHooks, JjCaptureError as E};
use crate::model::jj_observation::{
    JJ_OBSERVATION_READER_PROFILE, JjOperationEvidence, MAX_JJ_OBSERVATION_OPERATION_BYTES,
};
use crate::operations::jj::checkout::{DecodedJjCheckout, MAX_CHECKOUT_BYTES, decode_checkout};
use crate::operations::jj::evidence::verify_evidence;
use crate::operations::jj::operation::decode_operation;
use crate::regular_file::read_regular_at;
use std::ffi::OsStr;

pub(super) fn checkout_bytes(
    directories: &DirectoryRegistry,
    working_copy: usize,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<Vec<u8>, E> {
    read_file(
        directories,
        working_copy,
        "checkout",
        MAX_CHECKOUT_BYTES,
        "checkout",
        budget,
        hooks,
    )
}

pub(super) fn checkout(
    raw: &[u8],
    budget: &CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<DecodedJjCheckout, E> {
    budget.check(hooks)?;
    let result = decode_checkout(JJ_OBSERVATION_READER_PROFILE, raw);
    budget.check(hooks)?;
    result.map_err(|error| E::caused("checkout", error))
}

pub(super) fn operation_pair(
    operation_id: &str,
    source: &BoundSource,
    directories: &DirectoryRegistry,
    anchor: bool,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<JjOperationEvidence, E> {
    let stage = if anchor { "evidence" } else { "checkout" };
    let maximum = if anchor {
        MAX_JJ_OBSERVATION_OPERATION_BYTES.min(budget.anchor_remaining())
    } else {
        MAX_JJ_OBSERVATION_OPERATION_BYTES
    };
    let operation_bytes = read_file(
        directories,
        source.operations,
        operation_id,
        maximum,
        stage,
        budget,
        hooks,
    )?;
    budget.check(hooks)?;
    let decoded = decode_operation(
        JJ_OBSERVATION_READER_PROFILE,
        operation_id,
        &operation_bytes,
    );
    budget.check(hooks)?;
    let decoded = decoded.map_err(|error| E::caused(stage, error))?;
    let view_bytes = read_file(
        directories,
        source.views,
        &decoded.view_id,
        maximum - operation_bytes.len(),
        stage,
        budget,
        hooks,
    )?;
    let evidence = JjOperationEvidence {
        operation_id: decoded.operation_id,
        parent_ids: decoded.parent_ids,
        view_id: decoded.view_id,
        operation_bytes,
        view_bytes,
    };
    budget.check(hooks)?;
    let proof = verify_evidence(JJ_OBSERVATION_READER_PROFILE, &evidence);
    budget.check(hooks)?;
    proof.map_err(|error| E::caused(stage, error))?;
    if anchor {
        budget.retain_anchor_bytes(evidence.operation_bytes.len() + evidence.view_bytes.len())?;
    }
    Ok(evidence)
}

pub(super) fn require_workspace(
    evidence: &JjOperationEvidence,
    checkout: &DecodedJjCheckout,
    budget: &CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<(), E> {
    budget.check(hooks)?;
    let proof = verify_evidence(JJ_OBSERVATION_READER_PROFILE, evidence);
    budget.check(hooks)?;
    let proof = proof.map_err(|error| E::caused("checkout", error))?;
    if proof.operation().operation_id != checkout.operation_id
        || !proof
            .view()
            .wc_commit_ids
            .contains_key(&checkout.workspace_name)
    {
        return Err(E::invalid(
            "checkout",
            "workspace is absent from its own operation's view",
        ));
    }
    Ok(())
}

fn read_file(
    directories: &DirectoryRegistry,
    parent: usize,
    name: &str,
    maximum: usize,
    stage: &'static str,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<Vec<u8>, E> {
    budget.check(hooks)?;
    let bytes = read_regular_at(
        directories.file(parent),
        OsStr::new(name),
        maximum,
        &mut budget.metadata,
    );
    budget.check(hooks)?;
    bytes.map_err(|error| E::caused(stage, error))
}
