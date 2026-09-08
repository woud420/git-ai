use super::{
    DurableNativeAdmission, JjNativeAdmissionError as E, NativeAdmissionCursor,
    NativeAdmissionExpectation, NativeAdmissionReceipt,
};
use crate::model::jj_observation::{is_root, validate_ids, validate_source};
use crate::model::repository::jj_observation_journal::native_admission::{
    NativeAdmissionCursor as StoredCursor, StoredAdmissionSnapshot, StoredNativeAdmission,
};
use crate::operations::jj::ancestry::{JjAncestryInput, JjHeadClosure, verify_ancestry_to_receipt};
use crate::operations::jj::baseline::MAX_JJ_BASELINE_HEADS;
use crate::operations::jj::baseline_persistence::{
    BaselineReceipt, verify_native_baseline_snapshot_ref,
};
use crate::operations::jj::registration::{RegisteredJjCurrentState, unix::saved};

pub(super) struct Closure {
    head_closures: Vec<JjHeadClosure>,
    reached_baseline_ids: Vec<String>,
    reaches_root: bool,
}

pub(super) fn expectation(expected: &NativeAdmissionExpectation<'_>) -> Result<(), E> {
    for id in [
        expected.source_id,
        expected.initialization_receipt_id,
        expected.baseline_id,
    ] {
        validate_source(id).map_err(|_| E::Input("invalid expected scope identity"))?;
    }
    if expected.generation >= i64::MAX as u64 {
        return Err(E::Input("expected admission generation cannot advance"));
    }
    validate_ids(expected.admitted_head_ids, MAX_JJ_BASELINE_HEADS)
        .map_err(|_| E::Input("invalid expected head identities"))?;
    if expected.admitted_head_ids.is_empty()
        || expected.admitted_head_ids.iter().any(|id| is_root(id))
    {
        return Err(E::Input("invalid expected head set"));
    }
    Ok(())
}

pub(super) fn scope(
    expected: &NativeAdmissionExpectation<'_>,
    registered: &RegisteredJjCurrentState,
) -> Result<(), E> {
    if expected.source_id != registered.source_id()
        || expected.initialization_receipt_id != registered.initialization_receipt_id()
        || expected.baseline_id != registered.baseline().receipt().baseline_id()
    {
        return Err(E::Input("expected scope differs from registration"));
    }
    Ok(())
}

/// Validate the latest packet even when the requested receipt is historical.
/// Otherwise a new valid write could conceal existing native-invalid evidence.
pub(super) fn snapshot(snapshot: &StoredAdmissionSnapshot) -> Result<Option<Closure>, E> {
    saved::validate_historical(&snapshot.registration)?;
    let baseline = verify_native_baseline_snapshot_ref(&snapshot.registration.native)?;
    let latest = snapshot
        .latest
        .as_ref()
        .map(|stored| packet(&baseline, stored))
        .transpose()?;
    let Some(requested) = snapshot.requested() else {
        return Ok(None);
    };
    if snapshot
        .latest
        .as_ref()
        .is_some_and(|stored| std::ptr::eq(stored, requested))
    {
        Ok(latest)
    } else {
        packet(&baseline, requested).map(Some)
    }
}

fn packet(baseline: &BaselineReceipt, stored: &StoredNativeAdmission) -> Result<Closure, E> {
    let record = &stored.record;
    let operations: Vec<_> = record.operations.iter().collect();
    let proof = verify_ancestry_to_receipt(
        baseline,
        JjAncestryInput {
            source_id: &record.source_id,
            reader_profile: &record.reader_profile,
            baseline_id: &record.baseline_id,
            expected_native_generation: record.baseline_generation,
            head_ids: &record.captured_head_ids,
            operations: &operations,
        },
    )?;
    if !proof
        .ordered_operations()
        .iter()
        .map(|item| item.evidence().operation_id.as_str())
        .eq(record
            .operations
            .iter()
            .map(|item| item.operation_id.as_str()))
    {
        return Err(E::Input(
            "saved evidence is not in deterministic ancestry order",
        ));
    }
    Ok(Closure {
        head_closures: proof.head_closures().to_vec(),
        reached_baseline_ids: proof.reached_baseline_ids().to_vec(),
        reaches_root: proof.reaches_root(),
    })
}

pub(super) fn cursor(
    registered: &RegisteredJjCurrentState,
    stored: StoredCursor,
) -> NativeAdmissionCursor {
    let baseline = registered.baseline().receipt();
    NativeAdmissionCursor {
        source_id: registered.source_id().to_owned(),
        initialization_receipt_id: registered.initialization_receipt_id().to_owned(),
        reader_profile: baseline.reader_profile().to_owned(),
        baseline_id: baseline.baseline_id().to_owned(),
        baseline_generation: baseline.generation(),
        generation: stored.generation,
        admitted_head_ids: stored.admitted_head_ids,
    }
}

pub(super) fn receipt(stored: &StoredNativeAdmission) -> NativeAdmissionReceipt {
    let record = &stored.record;
    NativeAdmissionReceipt {
        admission_id: stored.admission_id.clone(),
        cursor: NativeAdmissionCursor {
            source_id: record.source_id.clone(),
            initialization_receipt_id: record.initialization_receipt_id.clone(),
            reader_profile: record.reader_profile.clone(),
            baseline_id: record.baseline_id.clone(),
            baseline_generation: record.baseline_generation,
            generation: stored.generation,
            admitted_head_ids: record.captured_head_ids.clone(),
        },
        expected_generation: record.expected_admission_generation,
        expected_admitted_head_ids: record.expected_admitted_head_ids.clone(),
    }
}

pub(super) fn finish(stored: StoredNativeAdmission, closure: Closure) -> DurableNativeAdmission {
    DurableNativeAdmission {
        receipt: receipt(&stored),
        ordered_operations: stored.record.operations,
        head_closures: closure.head_closures,
        reached_baseline_ids: closure.reached_baseline_ids,
        reaches_root: closure.reaches_root,
    }
}
