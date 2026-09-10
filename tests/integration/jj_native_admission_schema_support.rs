use super::*;

pub(super) const FROZEN_V3: &str = include_str!("../fixtures/jj-observation-v3.sql");
pub(super) const ADMISSIONS: &str = "CREATE TABLE jj_native_admissions (
    source_id TEXT NOT NULL,
    admission_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, admission_id),
    UNIQUE (source_id, generation),
    FOREIGN KEY (source_id) REFERENCES jj_native_registrations(source_id)
);";
pub(super) const STATES: &str = "CREATE TABLE jj_native_admission_states (
    source_id TEXT PRIMARY KEY NOT NULL,
    admission_id TEXT NOT NULL,
    state BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, admission_id)
        REFERENCES jj_native_admissions(source_id, admission_id)
);";
pub(super) const TABLES: [&str; 2] = ["jj_native_admissions", "jj_native_admission_states"];

pub(super) fn frozen_v3() -> Fixture {
    let fixture = Fixture::new();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(FROZEN_V3).unwrap();
    fixture
}

pub(super) fn manual_v4(admissions: &str, states: &str) -> Fixture {
    let fixture = frozen_v3();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(admissions).unwrap();
    conn.execute_batch(states).unwrap();
    conn.execute(
        "UPDATE schema_metadata SET value = '4' WHERE key = 'version'",
        [],
    )
    .unwrap();
    fixture
}

pub(super) fn full_snapshot(conn: &Connection) -> Vec<Vec<Vec<Value>>> {
    let mut result = complete_snapshot(conn);
    for table in TABLES {
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

pub(super) fn reject_unchanged(fixture: &Fixture) {
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = full_snapshot(&conn);
    assert!(JjObservationJournal::open_at_path(&fixture.path).is_err());
    assert_eq!(full_snapshot(&conn), before);
}

pub(super) fn empty(conn: &Connection) {
    for table in TABLES {
        assert!(rows(conn, &format!("SELECT * FROM {table}")).is_empty());
    }
}

pub(super) fn shape(conn: &Connection) {
    assert_version(conn, "4");
    assert_columns(
        conn,
        "jj_native_admissions",
        &[
            ("source_id", "TEXT", 1),
            ("admission_id", "TEXT", 2),
            ("generation", "INTEGER", 0),
            ("record", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    );
    assert_columns(
        conn,
        "jj_native_admission_states",
        &[
            ("source_id", "TEXT", 1),
            ("admission_id", "TEXT", 0),
            ("state", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    );
    let mut expected = vec![
        index("pk", &["source_id", "admission_id"]),
        index("u", &["source_id", "generation"]),
    ];
    expected.sort();
    assert_eq!(index_shapes(conn, "jj_native_admissions"), expected);
    assert_eq!(
        index_shapes(conn, "jj_native_admission_states"),
        [index("pk", &["source_id"])]
    );
    let fk = |sequence, table: &str, from: &str, to: &str| {
        vec![
            Value::Integer(0),
            Value::Integer(sequence),
            Value::Text(table.to_owned()),
            Value::Text(from.to_owned()),
            Value::Text(to.to_owned()),
            Value::Text("NO ACTION".to_owned()),
            Value::Text("NO ACTION".to_owned()),
            Value::Text("NONE".to_owned()),
        ]
    };
    for (table, expected) in [
        (
            "jj_native_admissions",
            vec![fk(0, "jj_native_registrations", "source_id", "source_id")],
        ),
        (
            "jj_native_admission_states",
            vec![
                fk(0, "jj_native_admissions", "source_id", "source_id"),
                fk(1, "jj_native_admissions", "admission_id", "admission_id"),
            ],
        ),
    ] {
        let query = format!(
            "SELECT id, seq, \"table\", \"from\", \"to\", on_update, on_delete, match FROM pragma_foreign_key_list('{table}') ORDER BY id, seq"
        );
        assert_eq!(rows(conn, &query), expected);
    }
}

pub(super) fn altered(schema: &str, from: &str, to: &str) -> String {
    assert_eq!(
        schema.matches(from).count(),
        1,
        "ambiguous schema fixture edit"
    );
    schema.replacen(from, to, 1)
}

pub(super) fn explicit_generation_index() -> String {
    let table = altered(ADMISSIONS, ",\n    UNIQUE (source_id, generation)", "");
    format!(
        "{table}\nCREATE UNIQUE INDEX admission_generation ON jj_native_admissions(source_id, generation);"
    )
}
