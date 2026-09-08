use super::*;
use crate::model::repository::jj_observation_journal::JjObservationJournal;
use crate::model::repository::sqlite::open_with_memory_limits;
use rusqlite::Connection;

fn qualified_schema(default_nocase: bool) -> (tempfile::TempDir, Connection) {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("plans.sqlite");
    drop(JjObservationJournal::open_at_path(&path).unwrap());
    let conn = open_with_memory_limits(&path).unwrap();
    if default_nocase {
        conn.execute_batch(
            "DROP TABLE jj_native_workspaces; DROP TABLE jj_native_registrations;
             CREATE TABLE jj_native_registrations (
               source_id TEXT PRIMARY KEY NOT NULL,
               baseline_id TEXT NOT NULL,
               source_root_key TEXT COLLATE NOCASE NOT NULL,
               record BLOB NOT NULL, checksum TEXT NOT NULL,
               UNIQUE(source_root_key COLLATE BINARY),
               FOREIGN KEY(source_id,baseline_id) REFERENCES jj_native_baselines(source_id,baseline_id)
             );
             CREATE TABLE jj_native_workspaces (
               source_id TEXT NOT NULL REFERENCES jj_native_registrations(source_id),
               workspace_name TEXT COLLATE NOCASE NOT NULL,
               locator_key TEXT COLLATE NOCASE NOT NULL,
               workspace_root_key TEXT NOT NULL,
               record BLOB NOT NULL, checksum TEXT NOT NULL,
               PRIMARY KEY(source_id,workspace_name COLLATE BINARY),
               UNIQUE(locator_key COLLATE BINARY), UNIQUE(source_id,workspace_root_key)
             );"
        ).unwrap();
        // This is an equivalent qualified shape, not a post-open weakened schema.
        drop(JjObservationJournal::open_at_path(&path).unwrap());
    }
    (home, conn)
}

fn assert_point_plan(conn: &Connection, sql: &str, table: &str, column: &str) {
    let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let details: Vec<String> = statement
        .query_map(["ab".repeat(32)], |row| row.get(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(details.len(), 1, "{sql}: {details:?}");
    let detail = &details[0];
    assert!(
        detail.starts_with(&format!("SEARCH {table} USING ")),
        "{detail}"
    );
    assert!(detail.contains("INDEX "), "{detail}");
    assert!(detail.contains(&format!("{column}=?")), "{detail}");
    assert!(!detail.contains("SCAN"), "{detail}");
    assert!(!detail.contains("AUTOMATIC"), "{detail}");
}

#[test]
fn registration_model_guard_and_gap_queries_use_existing_point_indexes() {
    for default_nocase in [false, true] {
        let (_home, conn) = qualified_schema(default_nocase);
        for (sql, table, column) in [
            (
                SOURCE_ROOT_GUARD_SQL,
                "jj_native_registrations",
                "source_root_key",
            ),
            (
                WORKSPACE_LOCATOR_GUARD_SQL,
                "jj_native_workspaces",
                "locator_key",
            ),
            (
                SOURCE_WORKSPACE_EXISTS_SQL,
                "jj_native_workspaces",
                "source_id",
            ),
            (
                SOURCE_WORKSPACE_COUNT_SQL,
                "jj_native_workspaces",
                "source_id",
            ),
            (NATIVE_STATE_COUNT_SQL, "jj_native_sources", "source_id"),
            (
                NATIVE_BASELINE_EXISTS_SQL,
                "jj_native_baselines",
                "source_id",
            ),
        ] {
            assert_point_plan(&conn, sql, table, column);
        }
    }
}
#[test]
fn registration_model_frozen_v3_complete_snapshot_is_qualified_before_migration() {
    let home = tempfile::tempdir().unwrap();
    let conn = open_with_memory_limits(home.path().join("frozen-v3.sqlite")).unwrap();
    conn.execute_batch(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/jj-observation-v3.sql"
    )))
    .unwrap();
    let source = "0000000000000000000000000000000000000000000000000000000000000001";
    let mut budget = ReadBudget::new(8 * 1024 * 1024 + 4 * 128 * 1024);
    let saved = snapshot(&conn, source, "default", &mut budget)
        .unwrap()
        .unwrap();
    assert_eq!(saved.registration.record.source_id, source);
    assert_eq!(saved.registration.record.initial_workspace_name, "default");
    assert_eq!(saved.original_workspace.record.workspace_name, "default");
    assert_eq!(saved.selected_workspace().record.workspace_name, "default");
    assert_eq!(
        saved.registration.record.initial_workspace_record_id,
        saved.original_workspace.checksum
    );
    assert_eq!(saved.native.state.generation, 1);
    assert_eq!(
        saved.native.state.baseline_id,
        "b78dbce79e7503edc15d8264431c6ccd851a68c67e84c0f390d499302c3f590a"
    );
    assert!(budget.consumed() > 0);
    let version: String = conn
        .query_row(
            "SELECT value FROM schema_metadata WHERE key='version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "3");
}
#[test]
fn registration_model_admission_orphans_cannot_be_treated_as_absent_source() {
    for statement in [
        "INSERT INTO jj_native_admissions VALUES (?1, 'orphan', 1, X'01', 'untrusted')",
        "INSERT INTO jj_native_admission_states VALUES (?1, 'orphan', X'01', 'untrusted')",
    ] {
        let (_home, conn) = qualified_schema(false);
        // This installs the future shape only on the pre-migration RED binary.
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='jj_native_admissions')",
            [], |row| row.get(0),
        ).unwrap();
        if !exists {
            conn.execute_batch(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/jj-admission-schema-v4.sql"
            )))
            .unwrap();
        }
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        let source = "a1".repeat(32);
        conn.execute(statement, [&source]).unwrap();
        assert!(require_absent(&conn, &source).is_err());
        let mut budget = ReadBudget::new(0);
        assert!(snapshot(&conn, &source, "default", &mut budget).is_err());
        assert_eq!(budget.consumed(), 0);
        let other = "b2".repeat(32);
        require_absent(&conn, &other).unwrap();
        assert!(
            snapshot(&conn, &other, "default", &mut budget)
                .unwrap()
                .is_none()
        );
        assert_eq!(budget.consumed(), 0);
    }
}
#[test]
fn registration_model_new_admission_presence_queries_are_indexed() {
    for default_nocase in [false, true] {
        let (_home, conn) = qualified_schema(default_nocase);
        for (sql, table) in [
            (NATIVE_ADMISSION_EXISTS_SQL, "jj_native_admissions"),
            (
                NATIVE_ADMISSION_STATE_EXISTS_SQL,
                "jj_native_admission_states",
            ),
        ] {
            assert_point_plan(&conn, sql, table, "source_id");
        }
    }
}
