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
