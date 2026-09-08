use super::*;
use crate::operations::jj::admission::unix::reconcile_with_hooks;
use crate::operations::jj::admission::{
    NativeReconciliationExpectation, NativeReconciliationOutcome, RegisteredNativeAdmissionState,
    admit_registered_history,
};

mod faults;
mod scope;
mod support;
use support::{
    UNCHANGED_PHASES, deny_native_writes, reconcile, replace_admitted_heads,
    replace_registration_receipt, setup, status, unchanged, writer_reservation,
};

#[test]
fn reconciliation_private_unchanged_keeps_the_immediate_reservation_without_staging() {
    for with_prior in [false, true] {
        let (case, mut journal, expected, latest) = setup(with_prior);
        deny_native_writes(&case.db);
        let sql = sql_snapshot(&case.db, true);
        let files = filesystem(&case.fixture);
        let mut hooks = Hook::new(|phase| match phase {
            AdmissionPhase::UnchangedCandidate => writer_reservation(&case.db, false),
            AdmissionPhase::UnchangedSnapshotVerified => writer_reservation(&case.db, true),
            _ => panic!("unchanged reconciliation entered the admission path"),
        });
        let result = unchanged(reconcile(&case, &mut journal, &expected, &mut hooks).unwrap());
        assert_eq!(hooks.phases, UNCHANGED_PHASES);
        assert_eq!(result.cursor(), &expected);
        assert_eq!(
            result.registration().initialization_receipt_id(),
            case.registered.initialization_receipt_id()
        );
        assert_eq!(
            result
                .latest_receipt()
                .map(|receipt| receipt.admission_id()),
            latest.as_deref()
        );
        assert!(sql_snapshot(&case.db, true) == sql);
        assert!(filesystem(&case.fixture) == files);
        writer_reservation(&case.db, false);
    }
}

#[test]
fn reconciliation_private_candidate_rechecks_a_coherently_replaced_registration_receipt() {
    let (case, mut journal, expected, _) = setup(false);
    let files = filesystem(&case.fixture);
    let mut injected_sql = None;
    let mut replacement_receipt = None;
    let mut hooks = Hook::new(|phase| {
        assert_eq!(phase, AdmissionPhase::UnchangedCandidate);
        replace_registration_receipt(&case.db);
        let checked = status(&case);
        assert_eq!(checked.cursor().generation(), expected.generation());
        assert_eq!(
            checked.cursor().admitted_head_ids(),
            expected.admitted_head_ids()
        );
        assert_eq!(checked.cursor().source_id(), expected.source_id());
        assert_eq!(checked.cursor().baseline_id(), expected.baseline_id());
        assert_ne!(
            checked.cursor().initialization_receipt_id(),
            expected.initialization_receipt_id()
        );
        replacement_receipt = Some(checked.cursor().initialization_receipt_id().to_owned());
        injected_sql = Some(sql_snapshot(&case.db, true));
    });
    let error = refuse(reconcile(&case, &mut journal, &expected, &mut hooks));
    assert!(error.to_string().contains("scope"), "{error}");
    assert_eq!(hooks.phases, [AdmissionPhase::UnchangedCandidate]);
    drop(hooks);
    assert!(sql_snapshot(&case.db, true) == injected_sql.unwrap());
    assert!(filesystem(&case.fixture) == files);
    assert_eq!(
        status(&case).cursor().initialization_receipt_id(),
        replacement_receipt.unwrap()
    );
    writer_reservation(&case.db, false);
}

#[test]
fn reconciliation_private_candidate_cannot_ignore_a_concurrent_generation_advance() {
    let (case, mut journal, expected, _) = setup(false);
    let files = filesystem(&case.fixture);
    let mut concurrent_sql = None;
    let mut latest = None;
    let mut hooks = Hook::new(|phase| {
        assert_eq!(phase, AdmissionPhase::UnchangedCandidate);
        let mut other = JjObservationJournal::open_at_path(&case.db).unwrap();
        let admitted = admitted(
            admit_registered_history(
                &mut other,
                &case.fixture.context(),
                &case.config,
                expected.expectation(),
                deadline(),
                &mut reads(),
            )
            .unwrap(),
        );
        assert_eq!(admitted.current_cursor().generation(), 1);
        assert_eq!(
            admitted.current_cursor().admitted_head_ids(),
            expected.admitted_head_ids()
        );
        latest = Some(admitted.admission().receipt().admission_id().to_owned());
        concurrent_sql = Some(sql_snapshot(&case.db, true));
    });
    refuse(reconcile(&case, &mut journal, &expected, &mut hooks));
    assert_eq!(hooks.phases, [AdmissionPhase::UnchangedCandidate]);
    drop(hooks);
    assert!(sql_snapshot(&case.db, true) == concurrent_sql.unwrap());
    assert!(filesystem(&case.fixture) == files);
    let checked = status(&case);
    assert_eq!(checked.cursor().generation(), 1);
    assert_eq!(
        checked.latest_receipt().unwrap().admission_id(),
        latest.unwrap()
    );
    writer_reservation(&case.db, false);
}

#[test]
fn reconciliation_private_late_heads_keep_the_checked_historical_noop_result() {
    for at in UNCHANGED_PHASES {
        let (case, mut journal, expected, _) = setup(false);
        let record = fixtures::rich_parent();
        write_records(&case.fixture, std::slice::from_ref(&record));
        let sql = sql_snapshot(&case.db, true);
        let mut injected_files = None;
        let mut hooks = Hook::new(|phase| {
            if phase == at {
                case.fixture.set_heads(&[&record.operation_id]);
                injected_files = Some(filesystem(&case.fixture));
            }
        });
        let result = unchanged(reconcile(&case, &mut journal, &expected, &mut hooks).unwrap());
        assert_eq!(hooks.phases, UNCHANGED_PHASES);
        drop(hooks);
        assert_eq!(result.cursor(), &expected);
        assert!(result.latest_receipt().is_none());
        assert!(sql_snapshot(&case.db, true) == sql);
        assert!(filesystem(&case.fixture) == injected_files.unwrap());
        writer_reservation(&case.db, false);
    }
}

#[test]
fn reconciliation_private_changed_path_keeps_the_existing_admission_phases() {
    let (case, mut journal, expected, _) = setup(false);
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    case.select(&records, &records[1]);
    let mut hooks = Hook::new(|phase| {
        if phase == AdmissionPhase::ReadbackVerified {
            writer_reservation(&case.db, true);
            assert_eq!(admission_counts(&case.db), [0, 0]);
        }
    });
    let result = match reconcile(&case, &mut journal, &expected, &mut hooks).unwrap() {
        NativeReconciliationOutcome::Admission(outcome) => admitted(outcome),
        NativeReconciliationOutcome::Unchanged(_) => panic!("changed heads were skipped"),
    };
    assert_eq!(hooks.phases, ALL_PHASES);
    assert_packet(&result, &case, &records, &records[1].operation_id);
    assert_eq!(admission_counts(&case.db), [1, 1]);
    writer_reservation(&case.db, false);
}

#[test]
fn reconciliation_private_candidate_rechecks_heads_even_when_generation_is_unchanged() {
    let (case, mut journal, expected, _) = setup(true);
    let files = filesystem(&case.fixture);
    let mut injected_sql = None;
    let mut hooks = Hook::new(|phase| {
        assert_eq!(phase, AdmissionPhase::UnchangedCandidate);
        replace_admitted_heads(&case.db);
        let checked = status(&case);
        assert_eq!(checked.cursor().generation(), expected.generation());
        assert_eq!(
            checked.cursor().initialization_receipt_id(),
            expected.initialization_receipt_id()
        );
        assert_ne!(
            checked.cursor().admitted_head_ids(),
            expected.admitted_head_ids()
        );
        assert_eq!(checked.cursor().admitted_head_ids().len(), 2);
        injected_sql = Some(sql_snapshot(&case.db, true));
    });
    refuse(reconcile(&case, &mut journal, &expected, &mut hooks));
    assert_eq!(hooks.phases, [AdmissionPhase::UnchangedCandidate]);
    drop(hooks);
    assert!(sql_snapshot(&case.db, true) == injected_sql.unwrap());
    assert!(filesystem(&case.fixture) == files);
    writer_reservation(&case.db, false);
}
