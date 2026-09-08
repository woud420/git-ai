use super::super::{
    JjRegisteredCheckoutRelation, JjRegistrationError as E, RegisteredJjCurrentState,
};
use crate::model::repository::jj_observation_journal::registration::{
    BaselineRelation, StoredRegistrationSnapshot, WorkspaceRecord,
};
use crate::operations::jj::baseline_persistence::verify_native_baseline_snapshot;
use crate::operations::jj::capture::CapturedJjCurrentState;
use crate::operations::jj::capture::registration::RetainedCapture;
use crate::operations::jj::checkout::decode_checkout;
use crate::operations::jj::evidence::verify_evidence;

pub(super) fn validate(
    current: &RetainedCapture<'_>,
    snapshot: &StoredRegistrationSnapshot,
) -> Result<(), E> {
    let seal = current
        .seal()
        .ok_or_else(|| E::invalid("seal", "registered source seal is absent"))?;
    let source = &snapshot.registration.record;
    let selected = &snapshot.selected_workspace().record;
    let locator = current
        .workspace_locator()
        .map_err(|error| E::caused("workspace locator", error))?;
    if source.source_id != seal.source_id()
        || source.seal_bytes.0 != seal.bytes()
        || source.source_binding != current.source_binding()
        || selected.workspace_name != current.captured().checkout().workspace_name
        || selected.locator != locator
        || selected.workspace_binding != current.workspace_binding()
    {
        return Err(E::invalid(
            "saved binding",
            "registration differs from the sampled source or workspace",
        ));
    }
    validate_historical_checkout(&snapshot.original_workspace.record, snapshot)?;
    if snapshot.original_workspace.record.workspace_name != selected.workspace_name {
        validate_historical_checkout(selected, snapshot)?;
    }
    Ok(())
}

fn validate_historical_checkout(
    workspace: &WorkspaceRecord,
    snapshot: &StoredRegistrationSnapshot,
) -> Result<(), E> {
    let selected = &workspace.selected_checkout;
    let checkout = decode_checkout(&workspace.reader_profile, &selected.raw_checkout_bytes.0)
        .map_err(|error| E::caused("saved checkout", error))?;
    if checkout.workspace_name != workspace.workspace_name
        || checkout.operation_id != selected.operation_id
    {
        return Err(E::invalid(
            "saved checkout",
            "checkout does not match its registration context",
        ));
    }
    let anchor = snapshot
        .native
        .record
        .anchors
        .iter()
        .find(|anchor| anchor.operation_id == selected.operation_id);
    match (&selected.baseline_relation, anchor) {
        (BaselineRelation::BaselineAnchor, Some(anchor)) => {
            let verified = verify_evidence(&workspace.reader_profile, anchor)
                .map_err(|error| E::caused("saved checkout anchor", error))?;
            if anchor.view_id != selected.view_id
                || !verified
                    .view()
                    .wc_commit_ids
                    .contains_key(&workspace.workspace_name)
            {
                return Err(E::invalid(
                    "saved checkout",
                    "workspace is absent from its saved anchor view",
                ));
            }
        }
        (BaselineRelation::OutsideBaseline, None) => {}
        _ => {
            return Err(E::invalid(
                "saved checkout",
                "checkout relation differs from the saved cutoff",
            ));
        }
    }
    Ok(())
}

pub(super) fn finish(
    snapshot: StoredRegistrationSnapshot,
    captured: &CapturedJjCurrentState,
) -> Result<RegisteredJjCurrentState, E> {
    let selected = &snapshot.selected_workspace().record;
    let source_id = snapshot.registration.record.source_id.clone();
    let initialization_receipt_id = snapshot.registration.checksum.clone();
    let workspace_name = selected.workspace_name.clone();
    let attachment_id = selected.attachment_id.clone();
    let checkout = captured.checkout().clone();
    let baseline = verify_native_baseline_snapshot(snapshot.native)
        .map_err(|error| E::caused("saved baseline", error))?;
    let checkout_relation = if baseline
        .anchors()
        .iter()
        .any(|anchor| anchor.operation_id == checkout.operation_id)
    {
        JjRegisteredCheckoutRelation::BaselineAnchor
    } else {
        JjRegisteredCheckoutRelation::OutsideBaseline
    };
    Ok(RegisteredJjCurrentState {
        source_id,
        initialization_receipt_id,
        workspace_name,
        attachment_id,
        baseline,
        checkout,
        checkout_relation,
    })
}
