use super::*;

#[path = "jj_journal_existing_rejections.rs"]
mod rejections;
#[path = "jj_journal_existing_wal.rs"]
mod wal;

fn wal_connection(fixture: &Fixture) -> Connection {
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let mode: String = conn
        .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    conn
}

fn reject_unchanged(fixture: &Fixture, conn: &Connection) {
    let before = readonly_snapshot(conn);
    let mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    // The journal is in TestRepo's separate home, outside this file snapshot.
    assert!(!fixture.path.starts_with(fixture._repo.path()));
    let repo_before = crate::debug_context::snapshot(fixture._repo.path());
    let sentinel = fixture.path.parent().unwrap().join("unrelated-sentinel");
    fs::write(&sentinel, b"preserve unrelated home data").unwrap();
    assert!(JjObservationJournal::open_existing_at_path(&fixture.path).is_err());
    assert_eq!(readonly_snapshot(conn), before);
    let after_mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(after_mode, mode);
    assert_eq!(
        crate::debug_context::snapshot(fixture._repo.path()),
        repo_before
    );
    assert_eq!(fs::read(sentinel).unwrap(), b"preserve unrelated home data");
}

#[test]
fn jj_journal_existing_wal_preserves_saved_records_and_accepts_capture() {
    let fixture = manual_v4(ADMISSIONS, STATES);
    let conn = wal_connection(&fixture);
    shape(&conn);
    let before = readonly_snapshot(&conn);
    let saved_native = registered_payload_snapshot(&conn);
    let opaque_tables = opaque_snapshot(&conn).len();
    let mut journal = JjObservationJournal::open_existing_at_path(&fixture.path).unwrap();
    fixture.assert_only_first(&journal);
    assert_eq!(readonly_snapshot(&conn), before);
    let baseline = baseline_reads::reopen(&journal, &fixture.source)
        .unwrap()
        .unwrap();
    assert_eq!(baseline.anchors(), [first()]);
    assert_eq!(baseline.receipt().generation(), 1);
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
    let mut budget = ReadBudget::new(source.len() + workspace.len());
    assert_eq!(
        journal
            .read_native_registration_record(&fixture.source, &mut budget)
            .unwrap(),
        Some(source)
    );
    assert_eq!(
        journal
            .read_native_workspace_record(&fixture.source, "default", &mut budget)
            .unwrap(),
        Some(workspace)
    );
    assert_eq!(budget.remaining(), 0);
    let request = batch(&fixture.source, 1, &[1], &[2], vec![evidence(2, &[1])]);
    assert_eq!(
        journal.capture(&request).unwrap(),
        CaptureOutcome::Captured {
            inserted_operations: 1
        }
    );
    drop(journal);
    let reopened = JjObservationJournal::open_existing_at_path(&fixture.path).unwrap();
    assert_eq!(reopened.status(&fixture.source).unwrap().generation, 2);
    assert_eq!(
        reopened.pending(&fixture.source, 2).unwrap(),
        [evidence(1, &[0]), evidence(2, &[1])]
    );
    // Opaque capture is an independent namespace; native baseline and receipts stay fixed.
    assert_eq!(
        &registered_payload_snapshot(&conn)[opaque_tables..],
        &saved_native[opaque_tables..]
    );
}

#[cfg(unix)]
#[path = "jj_journal_existing_permissions.rs"]
mod permissions;
