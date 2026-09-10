use super::{
    AdmissionHooks, AdmissionPhase, E, JjObservationJournal, NativeAdmissionExpectation,
    ReadBudget, RegisteredJjCurrentState, RegisteredNativeAdmissionState, RetainedCapture,
    check_deadline, expected::ExpectedAttempt, saved, verify,
};
use crate::model::repository::jj_observation_journal::registration::StoredRegistrationSnapshot;
use std::time::Instant;

pub(super) fn same_heads(expected: &[String], canonical: &[String]) -> bool {
    let mut expected: Vec<_> = expected.iter().collect();
    expected.sort_unstable();
    expected.into_iter().eq(canonical.iter())
}

pub(super) fn finish(
    journal: &mut JjObservationJournal,
    current: &mut RetainedCapture<'_>,
    registered: RegisteredJjCurrentState,
    expected: ExpectedAttempt<'_>,
    deadline: Instant,
    reads: &mut ReadBudget,
    hooks: &mut impl AdmissionHooks,
) -> Result<RegisteredNativeAdmissionState, E> {
    let expected_admission = &expected.admission;
    hooks.phase(AdmissionPhase::UnchangedCandidate);
    check_deadline(deadline, hooks)?;
    let transaction = journal.begin_native_admission(
        expected_admission.source_id,
        Some(&current.captured().checkout().workspace_name),
        None,
        reads,
    )?;
    let snapshot = transaction.snapshot();
    saved::validate(current, &snapshot.registration)?;
    verify::snapshot(snapshot)?;
    require_initial_scope(expected_admission, &registered, &snapshot.registration)?;
    let selected = &snapshot.registration.selected_workspace().record;
    expected.require_target(&selected.workspace_name, &selected.attachment_id)?;
    if expected_admission.generation != snapshot.cursor.generation
        || !same_heads(
            expected_admission.admitted_head_ids,
            &snapshot.cursor.admitted_head_ids,
        )
    {
        return Err(E::Input(
            "expected admission cursor differs from saved progress",
        ));
    }
    let result = RegisteredNativeAdmissionState {
        cursor: verify::cursor(&registered, snapshot.cursor.clone()),
        latest_receipt: snapshot.latest.as_ref().map(verify::receipt),
        registration: registered,
    };
    hooks.phase(AdmissionPhase::UnchangedSnapshotVerified);
    check_deadline(deadline, hooks)?;
    current.final_recheck()?;
    check_deadline(deadline, hooks)?;
    drop(transaction);
    Ok(result)
}

fn require_initial_scope(
    expected: &NativeAdmissionExpectation<'_>,
    initial: &RegisteredJjCurrentState,
    snapshot: &StoredRegistrationSnapshot,
) -> Result<(), E> {
    let source = &snapshot.registration.record;
    let state = &snapshot.native.state;
    let baseline = initial.baseline().receipt();
    let selected = &snapshot.selected_workspace().record;
    // Canonical receipt and baseline digests bind the raw saved evidence; the
    // selected attachment is separate and must also match this attempt's first read.
    if expected.source_id != source.source_id
        || expected.initialization_receipt_id != snapshot.registration.checksum
        || expected.baseline_id != state.baseline_id
        || initial.source_id() != source.source_id
        || initial.initialization_receipt_id() != snapshot.registration.checksum
        || baseline.source_id() != state.source_id
        || baseline.reader_profile() != state.reader_profile
        || baseline.baseline_id() != state.baseline_id
        || baseline.generation() != state.generation
        || baseline.captured_head_ids() != state.captured_head_ids
        || initial.workspace_name() != selected.workspace_name
        || initial.attachment_id() != selected.attachment_id
    {
        return Err(E::Input(
            "transactional scope differs from initial registration",
        ));
    }
    Ok(())
}
