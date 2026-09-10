use super::*;
use git_ai::model::repository::jj_observation_journal::ReadBudget;

#[path = "jj_native_admission_schema_drift.rs"]
mod drift;
#[path = "jj_native_admission_schema_support.rs"]
mod support;
#[path = "jj_native_admission_schema_versions.rs"]
mod versions;
use support::*;

#[test]
fn jj_admission_schema_frozen_v3_existing_records_are_compatible() {
    let fixture = frozen_v3();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    assert_version(&conn, "3");
    let before = registered_payload_snapshot(&conn);
    let mut journal = fixture.open();
    assert_existing_native_and_opaque(&fixture, &mut journal);
    let expected_source: Vec<u8> = conn
        .query_row(
            "SELECT record FROM jj_native_registrations WHERE source_id=?1",
            [&fixture.source],
            |row| row.get(0),
        )
        .unwrap();
    let expected_workspace: Vec<u8> = conn.query_row("SELECT record FROM jj_native_workspaces WHERE source_id=?1 AND workspace_name='default'", [&fixture.source], |row| row.get(0)).unwrap();
    let mut budget = ReadBudget::new(256 * 1024);
    assert_eq!(
        journal
            .read_native_registration_record(&fixture.source, &mut budget)
            .unwrap(),
        Some(expected_source)
    );
    assert_eq!(
        journal
            .read_native_workspace_record(&fixture.source, "default", &mut budget)
            .unwrap(),
        Some(expected_workspace)
    );
    assert_eq!(registered_payload_snapshot(&conn), before);
    assert!(rows(&conn, "PRAGMA foreign_key_check").is_empty());
}

#[test]
fn jj_admission_schema_fresh_database_has_exact_v4_shape_without_backfill() {
    let fixture = Fixture::new();
    drop(fixture.open());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    shape(&conn);
    empty(&conn);
    assert_native_tables_empty(&conn);
    assert_registration_tables_empty(&conn);
    assert!(opaque_snapshot(&conn).iter().all(Vec::is_empty));
}

#[test]
fn jj_admission_schema_v3_upgrade_preserves_every_old_payload_and_receipt() {
    assert!(FROZEN_V3.starts_with(FROZEN_V2));
    let fixture = frozen_v3();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = registered_payload_snapshot(&conn);
    assert_existing_native_and_opaque(&fixture, &mut fixture.open());
    shape(&conn);
    empty(&conn);
    assert_eq!(registered_payload_snapshot(&conn), before);
    let latest = full_snapshot(&conn);
    drop(fixture.open());
    assert_eq!(full_snapshot(&conn), latest);
}

#[test]
fn jj_admission_schema_existing_v4_preserves_opaque_new_rows_without_decoding() {
    let fixture = manual_v4(ADMISSIONS, STATES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    conn.execute(
        "INSERT INTO jj_native_admissions VALUES (?1, 'historical-id', 1, X'001122', 'opaque')",
        [&fixture.source],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO jj_native_admission_states VALUES (?1, 'historical-id', X'334455', 'opaque')",
        [&fixture.source],
    )
    .unwrap();
    let before = full_snapshot(&conn);
    drop(fixture.open());
    assert_eq!(full_snapshot(&conn), before);
    shape(&conn);
}

#[test]
fn jj_admission_schema_scoped_keys_and_foreign_keys_are_enforced() {
    let fixture = manual_v4(ADMISSIONS, STATES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    conn.execute(
        "INSERT INTO jj_native_baselines VALUES ('source-b', 'base-b', X'', 'fixture')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO jj_native_registrations VALUES ('source-b', 'base-b', 'root-b', X'', 'fixture')", []).unwrap();
    let before = registered_payload_snapshot(&conn);
    let source = fixture.source.as_str();
    let insert = "INSERT INTO jj_native_admissions VALUES (?1, ?2, ?3, X'', 'fixture')";
    conn.execute(insert, rusqlite::params![source, "packet-a", 1])
        .unwrap();
    assert!(
        conn.execute(insert, rusqlite::params![source, "packet-a", 2])
            .is_err()
    );
    assert!(
        conn.execute(insert, rusqlite::params![source, "packet-b", 1])
            .is_err()
    );
    assert!(
        conn.execute(insert, rusqlite::params!["missing-source", "packet-a", 1])
            .is_err()
    );
    conn.execute(insert, rusqlite::params![source, "packet-b", 2])
        .unwrap();
    conn.execute(insert, rusqlite::params!["source-b", "packet-a", 1])
        .unwrap();
    let state = "INSERT INTO jj_native_admission_states VALUES (?1, ?2, X'', 'fixture')";
    assert!(conn.execute(state, [source, "missing-packet"]).is_err());
    conn.execute(state, [source, "packet-a"]).unwrap();
    conn.execute(state, ["source-b", "packet-a"]).unwrap();
    assert!(conn.execute(state, [source, "packet-b"]).is_err());
    assert!(conn.execute(state, ["missing-source", "packet-a"]).is_err());
    assert!(
        conn.execute(
            "DELETE FROM jj_native_admissions WHERE admission_id='packet-a'",
            []
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "DELETE FROM jj_native_registrations WHERE source_id='source-b'",
            []
        )
        .is_err()
    );
    conn.execute(
        "UPDATE jj_native_admission_states SET admission_id='packet-b' WHERE source_id=?1",
        [source],
    )
    .unwrap();
    assert!(
        conn.execute(
            "UPDATE jj_native_admission_states SET admission_id='missing-packet'",
            []
        )
        .is_err()
    );
    assert_eq!(registered_payload_snapshot(&conn), before);
    assert!(rows(&conn, "PRAGMA foreign_key_check").is_empty());
}

#[test]
fn jj_admission_schema_equivalent_explicit_unique_index_is_supported() {
    let fixture = manual_v4(&explicit_generation_index(), STATES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let mut expected = vec![
        index("pk", &["source_id", "admission_id"]),
        index("c", &["source_id", "generation"]),
    ];
    expected.sort();
    assert_eq!(index_shapes(&conn, "jj_native_admissions"), expected);
    let before = full_snapshot(&conn);
    drop(fixture.open());
    assert_eq!(full_snapshot(&conn), before);
}

#[test]
fn jj_admission_schema_concurrent_v3_initializers_publish_complete_v4() {
    let fixture = frozen_v3();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = registered_payload_snapshot(&conn);
    let barrier = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| {
        let workers = (0..2)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let path = fixture.path.clone();
                scope.spawn(move || {
                    barrier.wait();
                    drop(open_after_contention(&path));
                    let conn = open_with_memory_limits(&path).unwrap();
                    shape(&conn);
                    empty(&conn);
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
    });
    assert_eq!(registered_payload_snapshot(&conn), before);
}

#[path = "jj_journal_readonly.rs"]
mod readonly;
