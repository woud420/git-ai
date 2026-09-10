use super::*;
use crate::jj_baseline_persistence::support as baseline_reads;
use crate::jj_evidence::support::first;
use std::fs;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "jj_journal_readonly_admission.rs"]
mod admission;
#[path = "jj_journal_readonly_rejections.rs"]
mod rejections;
#[path = "jj_journal_readonly_wal.rs"]
mod wal;

fn readonly_snapshot(conn: &Connection) -> Vec<Vec<Vec<Value>>> {
    let mut result = vec![rows(
        conn,
        "SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name",
    )];
    for table in [
        "schema_metadata",
        "ignored_metadata",
        "jj_sources",
        "jj_operations",
        "jj_batches",
        "jj_views",
        "jj_native_baselines",
        "jj_native_sources",
        "jj_native_registrations",
        "jj_native_workspaces",
        "jj_native_admissions",
        "jj_native_admission_states",
        "unrelated",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        if exists {
            result.push(rows(conn, &format!("SELECT * FROM {table} ORDER BY rowid")));
        }
    }
    result
}

fn assert_readonly_rejected_unchanged(fixture: &Fixture) {
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = readonly_snapshot(&conn);
    assert!(JjObservationJournal::open_read_only_at_path(&fixture.path).is_err());
    assert!(
        readonly_snapshot(&conn) == before,
        "read-only rejection changed logical data"
    );
}

#[test]
fn jj_journal_readonly_repeated_reads_preserve_opaque_native_and_registration_records() {
    let fixture = manual_v4(ADMISSIONS, STATES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    shape(&conn);
    let before = readonly_snapshot(&conn);
    let source: Vec<u8> = conn
        .query_row("SELECT record FROM jj_native_registrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    let workspace: Vec<u8> = conn
        .query_row("SELECT record FROM jj_native_workspaces", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
    for _ in 0..2 {
        let journal = JjObservationJournal::open_read_only_at_path(&fixture.path).unwrap();
        for _ in 0..2 {
            fixture.assert_only_first(&journal);
            let baseline = baseline_reads::reopen(&journal, &fixture.source)
                .unwrap()
                .unwrap();
            assert_eq!(baseline.anchors(), [first()]);
            assert_eq!(baseline.receipt().generation(), 1);
            let mut budget = ReadBudget::new(source.len() + workspace.len());
            assert_eq!(
                journal
                    .read_native_registration_record(&fixture.source, &mut budget)
                    .unwrap(),
                Some(source.clone())
            );
            assert_eq!(
                journal
                    .read_native_workspace_record(&fixture.source, "default", &mut budget)
                    .unwrap(),
                Some(workspace.clone())
            );
            assert_eq!(budget.remaining(), 0);
        }
    }
    assert!(
        readonly_snapshot(&conn) == before,
        "reads changed logical data"
    );
    let after_mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(after_mode, mode);
}

#[test]
fn jj_journal_readonly_accepts_empty_current_schema_without_initializing_a_source() {
    let fixture = Fixture::new();
    drop(fixture.open());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = readonly_snapshot(&conn);
    let journal = JjObservationJournal::open_read_only_at_path(&fixture.path).unwrap();
    assert_eq!(journal.status(&fixture.source).unwrap().generation, 0);
    assert!(journal.pending(&fixture.source, 1).unwrap().is_empty());
    assert!(
        baseline_reads::reopen(&journal, &fixture.source)
            .unwrap()
            .is_none()
    );
    let mut budget = ReadBudget::new(0);
    assert!(
        journal
            .read_native_registration_record(&fixture.source, &mut budget)
            .unwrap()
            .is_none()
    );
    assert_eq!(budget.consumed(), 0);
    assert!(
        readonly_snapshot(&conn) == before,
        "absence reads installed a source"
    );
}

#[test]
fn jj_journal_readonly_refuses_a_valid_capture_that_the_writable_opener_accepts() {
    let fixture = manual_v4(ADMISSIONS, STATES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let request = batch(&fixture.source, 1, &[1], &[2], vec![evidence(2, &[1])]);
    let before = readonly_snapshot(&conn);
    let mut journal = JjObservationJournal::open_read_only_at_path(&fixture.path).unwrap();
    let error = journal.capture(&request).unwrap_err();
    assert!(
        matches!(
            error,
            JournalError::Persistence(PersistenceError::Sqlite {
                code: Some(rusqlite::ffi::ErrorCode::ReadOnly),
                ..
            })
        ),
        "unexpected refusal: {error}"
    );
    fixture.assert_only_first(&journal);
    assert!(
        readonly_snapshot(&conn) == before,
        "refused write changed logical data"
    );
    drop(journal);
    assert_eq!(
        fixture.open().capture(&request).unwrap(),
        CaptureOutcome::Captured {
            inserted_operations: 1
        }
    );
    let reopened = JjObservationJournal::open_read_only_at_path(&fixture.path).unwrap();
    assert_eq!(reopened.status(&fixture.source).unwrap().generation, 2);
}

#[path = "jj_journal_existing.rs"]
mod existing;
