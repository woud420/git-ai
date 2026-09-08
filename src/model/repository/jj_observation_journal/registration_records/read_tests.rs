use super::*;
use crate::model::repository::sqlite::open_with_memory_limits;
use rusqlite::{Connection, ToSql};

const PLAN_SCHEMA: &str = "
CREATE TABLE jj_native_baselines (
    source_id TEXT NOT NULL,
    baseline_id TEXT NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, baseline_id)
);
CREATE TABLE jj_native_registrations (
    source_id TEXT PRIMARY KEY NOT NULL,
    baseline_id TEXT NOT NULL,
    source_root_key TEXT NOT NULL UNIQUE,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, baseline_id)
        REFERENCES jj_native_baselines(source_id, baseline_id)
);
CREATE TABLE jj_native_workspaces (
    source_id TEXT NOT NULL REFERENCES jj_native_registrations(source_id),
    workspace_name TEXT NOT NULL,
    locator_key TEXT NOT NULL,
    workspace_root_key TEXT NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, workspace_name),
    UNIQUE (locator_key),
    UNIQUE (source_id, workspace_root_key)
);";

fn plan_fixture(default_nocase: bool) -> Connection {
    let conn = open_with_memory_limits(":memory:").unwrap();
    let schema = if default_nocase {
        PLAN_SCHEMA
            .replace(
                "workspace_name TEXT NOT NULL",
                "workspace_name TEXT COLLATE NOCASE NOT NULL",
            )
            .replace(
                "PRIMARY KEY (source_id, workspace_name)",
                "PRIMARY KEY (source_id, workspace_name COLLATE BINARY)",
            )
    } else {
        PLAN_SCHEMA.to_owned()
    };
    conn.execute_batch(&schema).unwrap();
    conn
}

fn assert_point_plan(
    conn: &Connection,
    sql: &str,
    parameters: &[&dyn ToSql],
    table: &str,
    columns: &[&str],
) {
    let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let details = statement
        .query_map(parameters, |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(details.len(), 1, "unexpected plan for {table}: {details:?}");
    let detail = &details[0];
    assert!(
        detail.starts_with(&format!("SEARCH {table} USING ")),
        "expected indexed point search: {detail}"
    );
    assert!(
        detail.contains(&format!("INDEX sqlite_autoindex_{table}_1 ")),
        "expected the declared primary-key index: {detail}"
    );
    for column in columns {
        assert!(
            detail.contains(&format!("{column}=?")),
            "missing exact indexed key {column}: {detail}"
        );
    }
    assert!(!detail.contains("SCAN"), "unexpected scan: {detail}");
    assert!(
        !detail.contains("AUTOMATIC"),
        "unexpected transient query-planner index: {detail}"
    );
}

fn assert_all_read_plans(conn: &Connection) {
    let source = "a".repeat(64);
    let name = "default";
    let selected_byte_limit = 128 * 1024_i64;
    assert_point_plan(
        conn,
        REGISTRATION_COUNT_SQL,
        &[&source],
        "jj_native_registrations",
        &["source_id"],
    );
    assert_point_plan(
        conn,
        WORKSPACE_COUNT_SQL,
        &[&source, &name],
        "jj_native_workspaces",
        &["source_id", "workspace_name"],
    );
    assert_point_plan(
        conn,
        REGISTRATION_PAYLOAD_SQL,
        &[&source, &selected_byte_limit],
        "jj_native_registrations",
        &["source_id"],
    );
    assert_point_plan(
        conn,
        WORKSPACE_PAYLOAD_SQL,
        &[&source, &name, &selected_byte_limit],
        "jj_native_workspaces",
        &["source_id", "workspace_name"],
    );
}

#[test]
fn native_registration_record_queries_use_primary_key_point_searches() {
    assert_all_read_plans(&plan_fixture(false));
}

#[test]
fn native_registration_record_queries_keep_binary_point_search_with_nocase_name_default() {
    assert_all_read_plans(&plan_fixture(true));
}
