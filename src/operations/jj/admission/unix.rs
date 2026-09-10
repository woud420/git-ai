use super::{
    DurableNativeAdmission, JjNativeAdmissionError as E, NativeAdmissionExpectation,
    NativeAdmissionOutcome, NativeReconciliationExpectation, NativeReconciliationOutcome,
    RegisteredNativeAdmission, RegisteredNativeAdmissionState, verify,
};
use crate::config::Config;
use crate::model::jj_observation::validate_source;
use crate::model::repository::jj_observation_journal::native_admission::{
    ExpectedAdmissionCursor, NativeAdmissionOutcome as StoredOutcome, NativeAdmissionScope,
    PreparedNativeAdmission,
};
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::jj::capture::registration::{
    BorrowedJjHistoryEvidence, HistoryCaptureBudget, RegistrationCaptureBudget, RetainedCapture,
};
use crate::operations::jj::registration::{
    RegisteredJjCurrentState,
    unix::{load_registered_context, policy, saved},
};
use crate::operations::workspace_context::WorkspaceContext;
use std::time::Instant;

mod expected;
mod unchanged;
use expected::ExpectedAttempt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdmissionPhase {
    HistoryCollected,
    PriorSnapshotVerified,
    ReadbackVerified,
    UnchangedCandidate,
    UnchangedSnapshotVerified,
}

pub(crate) trait AdmissionHooks {
    fn phase(&mut self, _phase: AdmissionPhase) {}
    fn now(&mut self) -> Instant {
        Instant::now()
    }
}

pub(super) struct Live;
impl AdmissionHooks for Live {}

fn check_deadline(deadline: Instant, hooks: &mut impl AdmissionHooks) -> Result<(), E> {
    policy::check_deadline(deadline)?;
    if hooks.now() >= deadline {
        return Err(E::Input("cooperative deadline elapsed"));
    }
    Ok(())
}

pub(crate) fn admit_with_hooks(
    journal: &mut JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    expected: NativeAdmissionExpectation<'_>,
    deadline: Instant,
    reads: &mut ReadBudget,
    hooks: &mut impl AdmissionHooks,
) -> Result<NativeAdmissionOutcome, E> {
    let expected = ExpectedAttempt::explicit(expected);
    check_request(config, &expected, deadline, hooks)?;
    let mut budget = HistoryCaptureBudget::new(deadline);
    let mut current = budget.open(context)?;
    let registered = load_initial(
        journal,
        &mut current,
        config,
        &expected,
        deadline,
        reads,
        hooks,
    )?;
    admit_captured(
        journal,
        &mut current,
        registered,
        expected,
        deadline,
        reads,
        hooks,
    )
}

pub(crate) fn reconcile_with_hooks(
    journal: &mut JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    expected: NativeReconciliationExpectation<'_>,
    deadline: Instant,
    reads: &mut ReadBudget,
    hooks: &mut impl AdmissionHooks,
) -> Result<NativeReconciliationOutcome, E> {
    let expected = ExpectedAttempt::reconciliation(expected);
    check_request(config, &expected, deadline, hooks)?;
    let mut budget = HistoryCaptureBudget::new(deadline);
    let mut current = budget.open(context)?;
    let registered = load_initial(
        journal,
        &mut current,
        config,
        &expected,
        deadline,
        reads,
        hooks,
    )?;
    if unchanged::same_heads(
        expected.admission.admitted_head_ids,
        current.captured().head_ids(),
    ) {
        unchanged::finish(
            journal,
            &mut current,
            registered,
            expected,
            deadline,
            reads,
            hooks,
        )
        .map(NativeReconciliationOutcome::Unchanged)
    } else {
        admit_captured(
            journal,
            &mut current,
            registered,
            expected,
            deadline,
            reads,
            hooks,
        )
        .map(NativeReconciliationOutcome::Admission)
    }
}

fn check_request(
    config: &Config,
    expected: &ExpectedAttempt<'_>,
    deadline: Instant,
    hooks: &mut impl AdmissionHooks,
) -> Result<(), E> {
    verify::expectation(&expected.admission)?;
    expected.validate_target()?;
    policy::require_opt_in(config)?;
    check_deadline(deadline, hooks)
}

fn load_initial(
    journal: &JjObservationJournal,
    current: &mut RetainedCapture<'_>,
    config: &Config,
    expected: &ExpectedAttempt<'_>,
    deadline: Instant,
    reads: &mut ReadBudget,
    hooks: &mut impl AdmissionHooks,
) -> Result<RegisteredJjCurrentState, E> {
    policy::authorize(current, config, deadline)?;
    let registered = load_registered_context(journal, current, deadline, reads)?;
    verify::scope(&expected.admission, &registered)?;
    expected.require_target(registered.workspace_name(), registered.attachment_id())?;
    check_deadline(deadline, hooks)?;
    Ok(registered)
}

fn admit_captured(
    journal: &mut JjObservationJournal,
    current: &mut RetainedCapture<'_>,
    registered: RegisteredJjCurrentState,
    expected: ExpectedAttempt<'_>,
    deadline: Instant,
    reads: &mut ReadBudget,
    hooks: &mut impl AdmissionHooks,
) -> Result<NativeAdmissionOutcome, E> {
    let expected_admission = &expected.admission;
    current.collect_history(registered.baseline())?;
    let baseline_generation = registered.baseline().receipt().generation();
    drop(registered);
    hooks.phase(AdmissionPhase::HistoryCollected);
    check_deadline(deadline, hooks)?;

    let history: BorrowedJjHistoryEvidence<'_> = current.borrow_history()?;
    let prepared = PreparedNativeAdmission::new(
        NativeAdmissionScope {
            source_id: expected_admission.source_id,
            reader_profile: current.captured().reader_profile(),
            initialization_receipt_id: expected_admission.initialization_receipt_id,
            baseline_id: expected_admission.baseline_id,
            baseline_generation,
        },
        ExpectedAdmissionCursor {
            generation: expected_admission.generation,
            admitted_head_ids: expected_admission.admitted_head_ids,
        },
        history.head_ids(),
        history.ordered_operations(),
    )?;
    let admission_id = prepared.admission_id().to_owned();
    check_deadline(deadline, hooks)?;
    let transaction = journal.begin_native_admission(
        expected_admission.source_id,
        Some(&current.captured().checkout().workspace_name),
        Some(&admission_id),
        reads,
    )?;
    saved::validate(current, &transaction.snapshot().registration)?;
    verify::snapshot(transaction.snapshot())?;
    let selected = &transaction
        .snapshot()
        .registration
        .selected_workspace()
        .record;
    expected.require_target(&selected.workspace_name, &selected.attachment_id)?;
    hooks.phase(AdmissionPhase::PriorSnapshotVerified);
    check_deadline(deadline, hooks)?;
    let staged = transaction.stage(prepared, reads)?;
    let (commit, outcome, snapshot) = staged.into_parts();
    saved::validate(current, &snapshot.registration)?;
    let closure = verify::snapshot(&snapshot)?.ok_or(E::Input(
        "staged admission has no verified requested packet",
    ))?;
    let (stored_registration, stored_cursor, requested) = snapshot.into_requested();
    let requested = requested.ok_or(E::Input("staged admission request is absent"))?;
    let admission = verify::finish(requested, closure);
    if admission.receipt().admission_id() != admission_id
        || admission.receipt().captured_head_ids() != history.head_ids()
        || admission.head_closures() != history.head_closures()
        || admission.reached_baseline_ids() != history.reached_baseline_ids()
        || admission.reaches_root() != history.reaches_root()
    {
        return Err(E::Input("saved admission differs from captured history"));
    }
    let registration = saved::finish(stored_registration, current.captured())?;
    verify::scope(expected_admission, &registration)?;
    expected.require_target(registration.workspace_name(), registration.attachment_id())?;
    let current_cursor = verify::cursor(&registration, stored_cursor);
    let value = RegisteredNativeAdmission {
        registration,
        admission,
        current_cursor,
    };
    let result = match outcome {
        StoredOutcome::Admitted => NativeAdmissionOutcome::Admitted(value),
        StoredOutcome::AlreadyAdmitted => NativeAdmissionOutcome::AlreadyAdmitted(value),
    };
    hooks.phase(AdmissionPhase::ReadbackVerified);
    check_deadline(deadline, hooks)?;
    drop(history);
    current.final_recheck()?;
    check_deadline(deadline, hooks)?;
    commit.commit()?;
    Ok(result)
}

pub(super) fn read_state(
    journal: &JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    deadline: Instant,
    reads: &mut ReadBudget,
) -> Result<RegisteredNativeAdmissionState, E> {
    policy::require_opt_in(config)?;
    let mut budget = RegistrationCaptureBudget::new(deadline);
    let mut current = budget.open_initial(context)?;
    policy::authorize(&mut current, config, deadline)?;
    let seal = current
        .seal()
        .ok_or(E::Input("registered source seal is absent"))?;
    policy::check_deadline(deadline)?;
    let snapshot = journal.read_native_admission_snapshot(
        seal.source_id(),
        Some(&current.captured().checkout().workspace_name),
        None,
        reads,
    )?;
    saved::validate(&current, &snapshot.registration)?;
    verify::snapshot(&snapshot)?;
    let latest_receipt = snapshot.latest.as_ref().map(verify::receipt);
    let registration = saved::finish(snapshot.registration, current.captured())?;
    let cursor = verify::cursor(&registration, snapshot.cursor);
    let result = RegisteredNativeAdmissionState {
        registration,
        cursor,
        latest_receipt,
    };
    policy::check_deadline(deadline)?;
    current.final_recheck()?;
    policy::check_deadline(deadline)?;
    Ok(result)
}

pub(super) fn read_known(
    journal: &JjObservationJournal,
    source_id: &str,
    admission_id: &str,
    deadline: Instant,
    reads: &mut ReadBudget,
) -> Result<Option<DurableNativeAdmission>, E> {
    validate_source(source_id).map_err(|_| E::Input("invalid source identity"))?;
    validate_source(admission_id).map_err(|_| E::Input("invalid admission identity"))?;
    policy::check_deadline(deadline)?;
    let snapshot =
        journal.read_native_admission_snapshot(source_id, None, Some(admission_id), reads)?;
    let closure = verify::snapshot(&snapshot)?;
    let (_, _, requested) = snapshot.into_requested();
    let result = match (requested, closure) {
        (Some(stored), Some(closure)) => Some(verify::finish(stored, closure)),
        (None, None) => None,
        _ => return Err(E::Input("saved admission verification is incomplete")),
    };
    policy::check_deadline(deadline)?;
    Ok(result)
}
