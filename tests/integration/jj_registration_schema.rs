use super::*;

#[path = "jj_registration_schema_drift.rs"]
mod drift;
#[path = "jj_registration_schema_support.rs"]
mod support;
#[path = "jj_registration_schema_versions.rs"]
mod versions;
use support::*;

#[path = "jj_native_admission_schema.rs"]
mod admission;

#[test]
fn jj_registration_schema_frozen_v2_native_and_opaque_receipts_are_compatible() {
    let fixture = frozen_v2();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    assert_version(&conn, "2");
    let before_native = native_snapshot(&conn);
    let before_opaque = opaque_snapshot(&conn);
    assert_existing_native_and_opaque(&fixture, &mut fixture.open());
    assert_eq!(native_snapshot(&conn), before_native);
    assert_eq!(opaque_snapshot(&conn), before_opaque);
}

#[test]
fn jj_registration_schema_fresh_database_has_exact_v3_keys_and_empty_registration() {
    let fixture = Fixture::new();
    drop(fixture.open());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    assert_latest_registration_shape(&conn);
    assert_registration_tables_empty(&conn);
    assert_native_tables_empty(&conn);
    assert!(opaque_snapshot(&conn).iter().all(Vec::is_empty));
}

#[test]
fn jj_registration_schema_v1_chain_preserves_frozen_opaque_bytes_without_backfill() {
    assert!(FROZEN_V2.starts_with(FROZEN_V1));
    let fixture = v1_fixture();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = opaque_snapshot(&conn);
    fixture.assert_only_first(&fixture.open());
    assert_latest_registration_shape(&conn);
    assert_registration_tables_empty(&conn);
    assert_native_tables_empty(&conn);
    assert_eq!(opaque_snapshot(&conn), before);
}

#[test]
fn jj_registration_schema_v2_upgrade_preserves_unregistered_native_baseline_exactly() {
    let fixture = frozen_v2();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before_native = native_snapshot(&conn);
    let before_opaque = opaque_snapshot(&conn);
    assert_existing_native_and_opaque(&fixture, &mut fixture.open());
    assert_latest_registration_shape(&conn);
    assert_registration_tables_empty(&conn);
    assert_eq!(native_snapshot(&conn), before_native);
    assert_eq!(opaque_snapshot(&conn), before_opaque);
    assert_eq!(
        before_native.iter().map(Vec::len).collect::<Vec<_>>(),
        [1, 1]
    );
}

#[test]
fn jj_registration_schema_v3_reopen_preserves_schema_rows_payloads_and_checksums() {
    let fixture = manual_v3(REGISTRATIONS, WORKSPACES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    // Schema opening does not decode or confer authority on registration records.
    conn.execute("INSERT INTO jj_native_registrations SELECT source_id, baseline_id, 'root-key', X'001122', 'historical registration' FROM jj_native_sources", []).unwrap();
    conn.execute("INSERT INTO jj_native_workspaces VALUES (?1, 'default', 'locator-key', 'workspace-root', X'334455', 'historical workspace')", [&fixture.source]).unwrap();
    let before = registered_payload_snapshot(&conn);
    assert_existing_native_and_opaque(&fixture, &mut fixture.open());
    assert_eq!(registered_payload_snapshot(&conn), before);
    let latest = complete_snapshot(&conn);
    drop(fixture.open());
    assert_eq!(complete_snapshot(&conn), latest);
    assert_latest_registration_shape(&conn);
}

#[test]
fn jj_registration_schema_keys_foreign_keys_and_scoped_workspace_root_are_enforced() {
    let fixture = Fixture::new();
    drop(fixture.open());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    let opaque_before = opaque_snapshot(&conn);
    for source in ["a", "b", "c"] {
        conn.execute(
            "INSERT INTO jj_native_baselines VALUES (?1, 'base', X'', 'fixture')",
            [source],
        )
        .unwrap();
    }
    let registration = "INSERT INTO jj_native_registrations VALUES (?1, ?2, ?3, X'', 'fixture')";
    conn.execute(registration, ["a", "base", "root-a"]).unwrap();
    conn.execute(registration, ["b", "base", "root-b"]).unwrap();
    assert!(
        conn.execute(registration, ["a", "base", "other-root"])
            .is_err()
    );
    assert!(conn.execute(registration, ["c", "base", "root-a"]).is_err());
    assert!(
        conn.execute(registration, ["c", "missing", "root-c"])
            .is_err()
    );
    let workspace = "INSERT INTO jj_native_workspaces VALUES (?1, ?2, ?3, ?4, X'', 'fixture')";
    conn.execute(workspace, ["a", "default", "locator-a", "workspace-root"])
        .unwrap();
    conn.execute(workspace, ["b", "default", "locator-b", "workspace-root"])
        .unwrap();
    assert!(
        conn.execute(
            workspace,
            ["a", "renamed", "moved-locator", "workspace-root"]
        )
        .is_err()
    );
    assert!(
        conn.execute(workspace, ["b", "linked", "locator-a", "other-root"])
            .is_err()
    );
    assert!(
        conn.execute(workspace, ["a", "default", "other-locator", "other-root"])
            .is_err()
    );
    assert!(
        conn.execute(workspace, ["c", "default", "locator-c", "root-c"])
            .is_err()
    );
    conn.execute(workspace, ["a", "Default", "case-locator", "case-root"])
        .unwrap();
    conn.execute(
        workspace,
        ["a", "workspace-工", "unicode-locator", "unicode-root"],
    )
    .unwrap();
    assert!(
        conn.execute(
            "DELETE FROM jj_native_registrations WHERE source_id = 'a'",
            []
        )
        .is_err()
    );
    assert!(
        conn.execute("DELETE FROM jj_native_baselines WHERE source_id = 'a'", [])
            .is_err()
    );
    assert!(rows(&conn, "PRAGMA foreign_key_check").is_empty());
    assert_eq!(opaque_snapshot(&conn), opaque_before);
}

#[test]
fn jj_registration_schema_concurrent_v2_openers_observe_complete_latest_schema() {
    let fixture = frozen_v2();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before_native = native_snapshot(&conn);
    let before_opaque = opaque_snapshot(&conn);
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
                    assert_latest_registration_shape(&conn);
                    assert_registration_tables_empty(&conn);
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
    });
    assert_eq!(native_snapshot(&conn), before_native);
    assert_eq!(opaque_snapshot(&conn), before_opaque);
}

#[path = "jj_registration_records.rs"]
mod records;
