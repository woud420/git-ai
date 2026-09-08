use super::*;
use git_ai::model::repository::error::PersistenceError;
use git_ai::model::repository::jj_observation_journal::JournalError;
use rusqlite::{Connection, types::Value};
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::time::Duration;

const FROZEN_V1: &str = include_str!("../fixtures/jj-observation-v1.sql");
const NATIVE_BASELINES: &str = "CREATE TABLE jj_native_baselines (
    source_id TEXT NOT NULL,
    baseline_id TEXT NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, baseline_id)
);";
const NATIVE_SOURCES: &str = "CREATE TABLE jj_native_sources (
    source_id TEXT PRIMARY KEY NOT NULL,
    baseline_id TEXT NOT NULL,
    state BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, baseline_id)
        REFERENCES jj_native_baselines(source_id, baseline_id)
);";

fn v1_fixture() -> Fixture {
    let fixture = Fixture::new();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(FROZEN_V1).unwrap();
    fixture
}

fn v2_fixture(baselines: &str, sources: &str) -> Fixture {
    let fixture = v1_fixture();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(baselines).unwrap();
    conn.execute_batch(sources).unwrap();
    conn.execute(
        "UPDATE schema_metadata SET value = '2' WHERE key = 'version'",
        [],
    )
    .unwrap();
    fixture
}

fn rows(conn: &Connection, query: &str) -> Vec<Vec<Value>> {
    let mut statement = conn.prepare(query).unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            (0..columns)
                .map(|index| row.get(index))
                .collect::<rusqlite::Result<Vec<Value>>>()
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn opaque_snapshot(conn: &Connection) -> Vec<Vec<Vec<Value>>> {
    [
        "SELECT source_id, state, checksum FROM jj_sources ORDER BY source_id",
        "SELECT source_id, operation_id, sequence, payload, checksum
            FROM jj_operations ORDER BY source_id, operation_id",
        "SELECT source_id, digest, receipt, checksum FROM jj_batches ORDER BY source_id, digest",
        "SELECT source_id, view_id, operation_id FROM jj_views ORDER BY source_id, view_id",
    ]
    .into_iter()
    .map(|query| rows(conn, query))
    .collect()
}

fn logical_snapshot(conn: &Connection) -> Vec<Vec<Vec<Value>>> {
    let mut snapshot = opaque_snapshot(conn);
    snapshot.push(rows(
        conn,
        "SELECT type, name, tbl_name, sql FROM sqlite_master
            WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    ));
    snapshot.push(rows(
        conn,
        "SELECT key, value FROM schema_metadata ORDER BY key",
    ));
    snapshot
}

fn assert_version(conn: &Connection, expected: &str) {
    assert_eq!(
        rows(
            conn,
            "SELECT typeof(value), value FROM schema_metadata WHERE key = 'version'"
        ),
        vec![vec![
            Value::Text("text".to_owned()),
            Value::Text(expected.to_owned())
        ]]
    );
}

fn assert_native_tables_empty(conn: &Connection) {
    for table in ["jj_native_baselines", "jj_native_sources"] {
        assert_eq!(
            rows(conn, &format!("SELECT count(*) FROM {table}")),
            vec![vec![Value::Integer(0)]]
        );
    }
}

fn assert_columns(conn: &Connection, table: &str, columns: &[(&str, &str, i64)]) {
    let actual = rows(conn, &format!("PRAGMA table_info('{table}')"));
    let expected: Vec<_> = columns
        .iter()
        .enumerate()
        .map(|(index, (name, kind, primary_key))| {
            vec![
                Value::Integer(index as i64),
                Value::Text((*name).to_owned()),
                Value::Text((*kind).to_owned()),
                Value::Integer(1),
                Value::Null,
                Value::Integer(*primary_key),
            ]
        })
        .collect();
    assert_eq!(actual, expected);
}

fn assert_opaque_wire_version_one(conn: &Connection) {
    for query in [
        "SELECT state FROM jj_sources",
        "SELECT payload FROM jj_operations",
        "SELECT receipt FROM jj_batches",
    ] {
        let bytes: Vec<u8> = conn.query_row(query, [], |row| row.get(0)).unwrap();
        let value: serde_json::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(value["schema_version"], 1);
    }
}

#[test]
fn jj_schema_frozen_v1_records_are_compatible_before_and_after_reopen() {
    let fixture = v1_fixture();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    assert_version(&conn, "1");
    let before = opaque_snapshot(&conn);
    let mut journal = fixture.open();
    fixture.assert_only_first(&journal);
    assert_eq!(
        journal.capture(&first_batch(&fixture.source)).unwrap(),
        CaptureOutcome::AlreadyCaptured
    );
    drop(journal);
    fixture.assert_only_first(&fixture.open());
    assert_eq!(opaque_snapshot(&conn), before);
    assert_opaque_wire_version_one(&conn);
}

#[test]
fn jj_schema_new_database_has_exact_native_v2_columns_and_no_native_rows() {
    let fixture = Fixture::new();
    drop(fixture.open());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    assert_version(&conn, "2");
    assert_columns(
        &conn,
        "jj_native_baselines",
        &[
            ("source_id", "TEXT", 1),
            ("baseline_id", "TEXT", 2),
            ("record", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    );
    assert_columns(
        &conn,
        "jj_native_sources",
        &[
            ("source_id", "TEXT", 1),
            ("baseline_id", "TEXT", 0),
            ("state", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    );
    assert_native_tables_empty(&conn);
    assert!(opaque_snapshot(&conn).iter().all(Vec::is_empty));
}

#[test]
fn jj_schema_v1_upgrade_preserves_opaque_bytes_order_and_historical_receipts() {
    let fixture = v1_fixture();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = opaque_snapshot(&conn);
    let mut journal = fixture.open();
    assert_version(&conn, "2");
    assert_native_tables_empty(&conn);
    assert_eq!(opaque_snapshot(&conn), before);
    assert_opaque_wire_version_one(&conn);
    journal
        .capture(&batch(
            &fixture.source,
            1,
            &[1],
            &[2],
            vec![evidence(2, &[1])],
        ))
        .unwrap();
    assert_eq!(
        journal.capture(&first_batch(&fixture.source)).unwrap(),
        CaptureOutcome::AlreadyCaptured
    );
    assert_eq!(
        journal.pending(&fixture.source, 2).unwrap(),
        vec![evidence(1, &[0]), evidence(2, &[1])]
    );
    assert_eq!(
        journal.status(&fixture.source).unwrap().observed_heads,
        vec![operation_id(2)]
    );
    assert!(
        journal
            .status(&fixture.source)
            .unwrap()
            .applied_heads
            .is_empty()
    );
    drop(journal);
    let reopened = fixture.open();
    assert_eq!(reopened.status(&fixture.source).unwrap().generation, 2);
    assert_native_tables_empty(&conn);
}

#[test]
fn jj_schema_existing_complete_v2_opens_without_rewriting_opaque_records() {
    let fixture = v2_fixture(NATIVE_BASELINES, NATIVE_SOURCES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = logical_snapshot(&conn);
    fixture.assert_only_first(&fixture.open());
    assert_eq!(logical_snapshot(&conn), before);
}

#[test]
fn jj_schema_native_keys_and_composite_reference_are_enforced_without_opaque_source() {
    let fixture = Fixture::new();
    drop(fixture.open());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    let before = opaque_snapshot(&conn);
    let baseline_insert =
        "INSERT INTO jj_native_baselines VALUES (?1, ?2, X'', 'opaque test payload')";
    let source_insert = "INSERT INTO jj_native_sources VALUES (?1, ?2, X'', 'opaque test payload')";
    conn.execute(baseline_insert, ["source-a", "baseline-a"])
        .unwrap();
    assert!(
        conn.execute(baseline_insert, ["source-a", "baseline-a"])
            .is_err()
    );
    conn.execute(baseline_insert, ["source-b", "baseline-a"])
        .unwrap();
    conn.execute(source_insert, ["source-a", "baseline-a"])
        .unwrap();
    assert!(
        conn.execute(source_insert, ["source-c", "baseline-a"])
            .is_err()
    );
    assert!(conn.execute(source_insert, ["source-b", "absent"]).is_err());
    conn.execute(baseline_insert, ["source-a", "baseline-b"])
        .unwrap();
    assert!(
        conn.execute(source_insert, ["source-a", "baseline-b"])
            .is_err()
    );
    assert!(
        conn.execute("UPDATE jj_native_sources SET baseline_id = 'absent'", [])
            .is_err()
    );
    assert!(conn.execute("DELETE FROM jj_native_baselines WHERE source_id = 'source-a' AND baseline_id = 'baseline-a'", []).is_err());
    assert_eq!(opaque_snapshot(&conn), before);
    assert!(rows(&conn, "PRAGMA foreign_key_check").is_empty());
    assert!(rows(&conn, "PRAGMA foreign_key_list('jj_native_baselines')").is_empty());
    let foreign_keys = rows(&conn, "PRAGMA foreign_key_list('jj_native_sources')");
    assert_eq!(foreign_keys.len(), 2);
    for (sequence, name) in ["source_id", "baseline_id"].into_iter().enumerate() {
        assert_eq!(
            foreign_keys[sequence],
            vec![
                Value::Integer(0),
                Value::Integer(sequence as i64),
                Value::Text("jj_native_baselines".to_owned()),
                Value::Text(name.to_owned()),
                Value::Text(name.to_owned()),
                Value::Text("NO ACTION".to_owned()),
                Value::Text("NO ACTION".to_owned()),
                Value::Text("NONE".to_owned()),
            ]
        );
    }
}

#[test]
fn jj_schema_v1_rejects_preexisting_native_tables_without_partial_upgrade() {
    for existing in [
        "CREATE TABLE jj_native_baselines (unexpected TEXT)".to_owned(),
        "CREATE TABLE jj_native_sources (unexpected TEXT)".to_owned(),
        NATIVE_BASELINES.to_owned(),
        NATIVE_SOURCES.to_owned(),
        format!("{NATIVE_BASELINES}\n{NATIVE_SOURCES}"),
    ] {
        let fixture = v1_fixture();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(&existing).unwrap();
        let before = logical_snapshot(&conn);
        assert!(JjObservationJournal::open_at_path(&fixture.path).is_err());
        assert_eq!(logical_snapshot(&conn), before);
        assert_version(&conn, "1");
    }
}

#[test]
fn jj_schema_v2_rejects_missing_tables_and_same_column_constraint_impostors() {
    let wrong_primary_key = NATIVE_BASELINES.replace(
        "PRIMARY KEY (source_id, baseline_id)",
        "PRIMARY KEY (source_id)",
    );
    let missing_primary_key =
        NATIVE_BASELINES.replace(",\n    PRIMARY KEY (source_id, baseline_id)", "");
    let nullable_identity = NATIVE_BASELINES.replace("source_id TEXT NOT NULL", "source_id TEXT");
    let wrong_blob_type = NATIVE_BASELINES.replace("record BLOB NOT NULL", "record TEXT NOT NULL");
    let missing_source_primary_key = NATIVE_SOURCES.replace(
        "source_id TEXT PRIMARY KEY NOT NULL",
        "source_id TEXT NOT NULL",
    );
    let missing_fk = "CREATE TABLE jj_native_sources (source_id TEXT PRIMARY KEY NOT NULL, baseline_id TEXT NOT NULL, state BLOB NOT NULL, checksum TEXT NOT NULL);";
    let swapped_fk = NATIVE_SOURCES.replace(
        "REFERENCES jj_native_baselines(source_id, baseline_id)",
        "REFERENCES jj_native_baselines(baseline_id, source_id)",
    );
    let cascade_fk = NATIVE_SOURCES.replace(
        "REFERENCES jj_native_baselines(source_id, baseline_id)",
        "REFERENCES jj_native_baselines(source_id, baseline_id) ON DELETE CASCADE",
    );
    for (baselines, sources) in [
        ("", NATIVE_SOURCES),
        (NATIVE_BASELINES, ""),
        (wrong_primary_key.as_str(), NATIVE_SOURCES),
        (missing_primary_key.as_str(), NATIVE_SOURCES),
        (nullable_identity.as_str(), NATIVE_SOURCES),
        (wrong_blob_type.as_str(), NATIVE_SOURCES),
        (NATIVE_BASELINES, missing_source_primary_key.as_str()),
        (NATIVE_BASELINES, missing_fk),
        (NATIVE_BASELINES, swapped_fk.as_str()),
        (NATIVE_BASELINES, cascade_fk.as_str()),
    ] {
        let fixture = v2_fixture(baselines, sources);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        let before = logical_snapshot(&conn);
        assert!(JjObservationJournal::open_at_path(&fixture.path).is_err());
        assert_eq!(logical_snapshot(&conn), before);
    }
}

#[test]
fn jj_schema_failed_version_update_rolls_back_native_ddl_and_opaque_data() {
    for timing in ["BEFORE", "AFTER"] {
        let fixture = v1_fixture();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TRIGGER fail_upgrade {timing} UPDATE ON schema_metadata
            WHEN OLD.key = 'version' AND OLD.value = '1' AND NEW.value = '2'
             AND (SELECT count(*) FROM sqlite_master WHERE type = 'table'
                  AND name IN ('jj_native_baselines', 'jj_native_sources')) = 2
            BEGIN SELECT RAISE(ABORT, 'injected migration failure'); END;"
        ))
        .unwrap();
        let before = logical_snapshot(&conn);
        assert!(JjObservationJournal::open_at_path(&fixture.path).is_err());
        assert_eq!(logical_snapshot(&conn), before);
        assert_version(&conn, "1");
        conn.execute_batch("DROP TRIGGER fail_upgrade").unwrap();
        fixture.assert_only_first(&fixture.open());
        assert_version(&conn, "2");
        assert_native_tables_empty(&conn);
    }
}

#[test]
fn jj_schema_malformed_or_unknown_version_remains_unchanged() {
    for value in [
        Value::Text("0".to_owned()),
        Value::Text("01".to_owned()),
        Value::Text("02".to_owned()),
        Value::Text("2\n".to_owned()),
        Value::Text("999".to_owned()),
        Value::Text("garbled".to_owned()),
        Value::Blob(b"1".to_vec()),
        Value::Blob(b"2".to_vec()),
    ] {
        let fixture = v1_fixture();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute(
            "UPDATE schema_metadata SET value = ?1 WHERE key = 'version'",
            [value],
        )
        .unwrap();
        let before = logical_snapshot(&conn);
        assert!(JjObservationJournal::open_at_path(&fixture.path).is_err());
        assert_eq!(logical_snapshot(&conn), before);
    }
}

#[test]
fn jj_schema_missing_version_or_opaque_table_is_not_repaired_by_upgrade() {
    for damage in [
        "DELETE FROM schema_metadata WHERE key = 'version'",
        "DROP TABLE schema_metadata",
        "DROP TABLE jj_views",
    ] {
        let fixture = v1_fixture();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        let before = opaque_snapshot(&conn);
        conn.execute_batch(damage).unwrap();
        let schema = rows(
            &conn,
            "SELECT name, sql FROM sqlite_master WHERE type = 'table' ORDER BY name",
        );
        assert!(JjObservationJournal::open_at_path(&fixture.path).is_err());
        assert_eq!(
            rows(
                &conn,
                "SELECT name, sql FROM sqlite_master WHERE type = 'table' ORDER BY name"
            ),
            schema
        );
        if damage != "DROP TABLE jj_views" {
            assert_eq!(opaque_snapshot(&conn), before);
        }
    }
}

fn open_after_contention(path: &Path) -> JjObservationJournal {
    for _ in 0..8 {
        match JjObservationJournal::open_at_path(path) {
            Ok(journal) => return journal,
            Err(JournalError::Persistence(PersistenceError::Sqlite {
                code:
                    Some(
                        rusqlite::ffi::ErrorCode::DatabaseBusy
                        | rusqlite::ffi::ErrorCode::DatabaseLocked,
                    ),
                ..
            })) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("concurrent initialization failed: {error}"),
        }
    }
    panic!("concurrent initialization exhausted bounded contention retries");
}

#[test]
fn jj_schema_concurrent_initializers_publish_only_complete_v2() {
    for populated in [false, true] {
        let fixture = if populated {
            v1_fixture()
        } else {
            Fixture::new()
        };
        let barrier = Arc::new(Barrier::new(2));
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..2)
                .map(|_| {
                    let barrier = Arc::clone(&barrier);
                    let path = fixture.path.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        drop(open_after_contention(&path));
                        let conn = open_with_memory_limits(&path).unwrap();
                        assert_version(&conn, "2");
                        assert_native_tables_empty(&conn);
                    })
                })
                .collect();
            for worker in workers {
                worker.join().unwrap();
            }
        });
        if populated {
            fixture.assert_only_first(&fixture.open());
        }
    }
}

#[test]
fn jj_schema_ignored_version_update_rolls_back_native_ddl() {
    let fixture = v1_fixture();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER ignore_upgrade BEFORE UPDATE ON schema_metadata
         WHEN OLD.key = 'version' AND OLD.value = '1' AND NEW.value = '2'
         BEGIN SELECT RAISE(IGNORE); END;",
    )
    .unwrap();
    let before = logical_snapshot(&conn);
    assert!(JjObservationJournal::open_at_path(&fixture.path).is_err());
    assert_eq!(logical_snapshot(&conn), before);
    assert_version(&conn, "1");
    conn.execute_batch("DROP TRIGGER ignore_upgrade").unwrap();
    fixture.assert_only_first(&fixture.open());
    assert_version(&conn, "2");
    assert_native_tables_empty(&conn);
}

#[test]
fn jj_schema_silently_rewritten_version_rolls_back_native_ddl() {
    for replacement in ["1", "999"] {
        let fixture = v1_fixture();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TRIGGER rewrite_upgrade AFTER UPDATE ON schema_metadata
             WHEN NEW.key = 'version' AND NEW.value = '2'
             BEGIN UPDATE schema_metadata SET value = '{replacement}' WHERE key = 'version'; END;"
        ))
        .unwrap();
        let before = logical_snapshot(&conn);
        assert!(JjObservationJournal::open_at_path(&fixture.path).is_err());
        assert_eq!(logical_snapshot(&conn), before);
        assert_version(&conn, "1");
        conn.execute_batch("DROP TRIGGER rewrite_upgrade").unwrap();
        fixture.assert_only_first(&fixture.open());
        assert_version(&conn, "2");
        assert_native_tables_empty(&conn);
    }
}

#[test]
fn jj_schema_duplicate_version_rows_reject_even_when_first_is_supported_v2() {
    for second_version in ["999", "2"] {
        let fixture = v2_fixture(NATIVE_BASELINES, NATIVE_SOURCES);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(
            "DROP TABLE schema_metadata;
             CREATE TABLE schema_metadata (key TEXT NOT NULL, value TEXT NOT NULL);
             INSERT INTO schema_metadata (key, value) VALUES ('version', '2');",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO schema_metadata (key, value) VALUES ('version', ?1)",
            [second_version],
        )
        .unwrap();
        let before = logical_snapshot(&conn);
        assert!(JjObservationJournal::open_at_path(&fixture.path).is_err());
        assert_eq!(logical_snapshot(&conn), before);
    }
}
