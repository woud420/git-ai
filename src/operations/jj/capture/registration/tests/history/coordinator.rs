use super::*;
use crate::config::Config;
use crate::config::tests::create_test_config;
use crate::model::repository::sqlite::open_with_memory_limits;
use crate::operations::jj::admission::unix::{AdmissionHooks, AdmissionPhase, admit_with_hooks};
use crate::operations::jj::admission::{
    JjNativeAdmissionError, NativeAdmissionCursor, NativeAdmissionOutcome,
    RegisteredNativeAdmission, read_registered_admission_state,
};
use crate::operations::jj::registration::{
    JjRegistrationOutcome, RegisteredJjCurrentState, register_current_state,
};
use rusqlite::types::Value as SqlValue;
use std::path::{Path, PathBuf};

mod faults;
mod native_baseline;
mod reconciliation;
mod support;
use support::*;

#[test]
fn admission_coordinator_registered_control_commits_after_all_three_phases() {
    let (case, mut journal, zero) = Case::new();
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    case.select(&records, &records[1]);
    let old = sql_snapshot(&case.db, false);
    let files = filesystem(&case.fixture);
    let mut hooks = Hook::new(|phase| {
        if phase == AdmissionPhase::ReadbackVerified {
            assert_eq!(admission_counts(&case.db), [0, 0]);
        }
    });
    let result = admitted(run(&case, &mut journal, &zero, &mut hooks).unwrap());
    assert_eq!(hooks.phases, ALL_PHASES);
    assert_packet(&result, &case, &records, &records[1].operation_id);
    assert!(sql_snapshot(&case.db, false) == old);
    assert!(filesystem(&case.fixture) == files);
    assert_eq!(admission_counts(&case.db), [1, 1]);
    let reopened = JjObservationJournal::open_at_path(&case.db).unwrap();
    let status = read_registered_admission_state(
        &reopened,
        &case.fixture.context(),
        &case.config,
        deadline(),
        &mut reads(),
    )
    .unwrap();
    assert_eq!(status.cursor(), result.current_cursor());
}

#[test]
fn admission_coordinator_late_heads_keep_the_completed_historical_sample() {
    let (case, mut journal, zero) = Case::new();
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    case.select(&records, &records[0]);
    let old = sql_snapshot(&case.db, false);
    let mut hooks = Hook::new(|phase| {
        if phase == AdmissionPhase::ReadbackVerified {
            case.fixture.set_heads(&[&records[1].operation_id]);
        }
    });
    let result = admitted(run(&case, &mut journal, &zero, &mut hooks).unwrap());
    assert_eq!(hooks.phases, ALL_PHASES);
    assert_packet(&result, &case, &records[..1], &records[0].operation_id);
    assert!(
        case.fixture
            .heads()
            .join(&records[1].operation_id)
            .is_file()
    );
    assert!(sql_snapshot(&case.db, false) == old);
}

#[test]
fn admission_coordinator_late_checkout_keeps_the_sampled_workspace_context() {
    let (case, mut journal, zero) = Case::new();
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    case.select(&records, &records[1]);
    set_checkout(&case.fixture, &records[0]);
    let old = sql_snapshot(&case.db, false);
    let mut hooks = Hook::new(|phase| {
        if phase == AdmissionPhase::ReadbackVerified {
            set_checkout(&case.fixture, &records[1]);
        }
    });
    let result = admitted(run(&case, &mut journal, &zero, &mut hooks).unwrap());
    assert_eq!(hooks.phases, ALL_PHASES);
    assert_packet(&result, &case, &records, &records[1].operation_id);
    assert_eq!(
        result.registration().checkout().operation_id,
        records[0].operation_id
    );
    assert!(sql_snapshot(&case.db, false) == old);
}

#[test]
fn admission_coordinator_gc_after_collection_does_not_reread_native_objects() {
    let (case, mut journal, zero) = Case::new();
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    case.select(&records, &records[1]);
    let old = sql_snapshot(&case.db, false);
    let mut hooks = Hook::new(|phase| {
        if phase == AdmissionPhase::HistoryCollected {
            for record in &records {
                fs::remove_file(operation_path(&case.fixture, &record.operation_id)).unwrap();
            }
            fs::remove_file(view_path(&case.fixture, &records[0].view_id)).unwrap();
        }
    });
    let result = admitted(run(&case, &mut journal, &zero, &mut hooks).unwrap());
    assert_eq!(hooks.phases, ALL_PHASES);
    assert_packet(&result, &case, &records, &records[1].operation_id);
    assert!(sql_snapshot(&case.db, false) == old);
    for record in &records {
        assert!(!operation_path(&case.fixture, &record.operation_id).exists());
    }
}
