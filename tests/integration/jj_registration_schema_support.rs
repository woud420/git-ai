use super::*;
use crate::jj_baseline_persistence::support as baseline_support;
use crate::jj_evidence::support::first;
use git_ai::operations::jj::baseline_persistence::BaselinePersistenceOutcome;

pub(super) const FROZEN_V2: &str = include_str!("../fixtures/jj-observation-v2.sql");
pub(super) const REGISTRATIONS: &str = "CREATE TABLE jj_native_registrations (
    source_id TEXT PRIMARY KEY NOT NULL,
    baseline_id TEXT NOT NULL,
    source_root_key TEXT NOT NULL UNIQUE,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, baseline_id)
        REFERENCES jj_native_baselines(source_id, baseline_id)
);";
pub(super) const WORKSPACES: &str = "CREATE TABLE jj_native_workspaces (
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

pub(super) fn frozen_v2() -> Fixture {
    let fixture = Fixture::new();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(FROZEN_V2).unwrap();
    fixture
}

pub(super) fn manual_v3(registrations: &str, workspaces: &str) -> Fixture {
    let fixture = frozen_v2();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(registrations).unwrap();
    conn.execute_batch(workspaces).unwrap();
    conn.execute(
        "UPDATE schema_metadata SET value = '3' WHERE key = 'version'",
        [],
    )
    .unwrap();
    fixture
}

pub(super) fn native_snapshot(conn: &Connection) -> Vec<Vec<Vec<Value>>> {
    [
        "SELECT source_id, baseline_id, record, checksum FROM jj_native_baselines ORDER BY source_id, baseline_id",
        "SELECT source_id, baseline_id, state, checksum FROM jj_native_sources ORDER BY source_id",
    ].into_iter().map(|query| rows(conn, query)).collect()
}

pub(super) fn complete_snapshot(conn: &Connection) -> Vec<Vec<Vec<Value>>> {
    let mut result = logical_snapshot(conn);
    for table in [
        "jj_native_baselines",
        "jj_native_sources",
        "jj_native_registrations",
        "jj_native_workspaces",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
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

pub(super) fn assert_registration_tables_empty(conn: &Connection) {
    for table in ["jj_native_registrations", "jj_native_workspaces"] {
        assert_eq!(
            rows(conn, &format!("SELECT count(*) FROM {table}")),
            vec![vec![Value::Integer(0)]]
        );
    }
}

pub(super) fn assert_existing_native_and_opaque(
    fixture: &Fixture,
    journal: &mut JjObservationJournal,
) {
    fixture.assert_only_first(journal);
    assert_eq!(
        journal.capture(&first_batch(&fixture.source)).unwrap(),
        CaptureOutcome::AlreadyCaptured
    );
    let baseline = baseline_support::reopen(journal, &fixture.source)
        .unwrap()
        .unwrap();
    assert_eq!(baseline.anchors(), [first()]);
    assert_eq!(baseline.receipt().generation(), 1);
    let retry = baseline_support::install(journal, &fixture.source, &[first()]).unwrap();
    let BaselinePersistenceOutcome::AlreadyInstalled(receipt) = retry else {
        panic!("frozen native receipt was not recognized")
    };
    assert_eq!(receipt, *baseline.receipt());
}

pub(super) fn assert_rejected_unchanged(fixture: &Fixture) {
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = complete_snapshot(&conn);
    assert!(JjObservationJournal::open_at_path(&fixture.path).is_err());
    assert_eq!(complete_snapshot(&conn), before);
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct IndexShape {
    pub origin: String,
    pub unique: bool,
    pub partial: bool,
    pub columns: Vec<(String, String, bool)>,
}

pub(super) fn index_shapes(conn: &Connection, table: &str) -> Vec<IndexShape> {
    let mut statement = conn
        .prepare("SELECT name, origin, \"unique\", partial FROM pragma_index_list(?1)")
        .unwrap();
    let indices = statement
        .query_map([table], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let mut result = Vec::new();
    for (name, origin, unique, partial) in indices {
        let mut columns = conn
            .prepare(
                "SELECT name, coll, desc FROM pragma_index_xinfo(?1) WHERE key = 1 ORDER BY seqno",
            )
            .unwrap();
        let columns = columns
            .query_map([name], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        result.push(IndexShape {
            origin,
            unique,
            partial,
            columns,
        });
    }
    result.sort();
    result
}

pub(super) fn index(origin: &str, columns: &[&str]) -> IndexShape {
    IndexShape {
        origin: origin.to_owned(),
        unique: true,
        partial: false,
        columns: columns
            .iter()
            .map(|column| ((*column).to_owned(), "BINARY".to_owned(), false))
            .collect(),
    }
}

pub(super) fn assert_v3_shape(conn: &Connection) {
    assert_version(conn, "3");
    assert_registration_shape(conn);
}

pub(super) fn assert_latest_registration_shape(conn: &Connection) {
    assert_version(conn, "4");
    assert_registration_shape(conn);
}

fn assert_registration_shape(conn: &Connection) {
    assert_columns(
        conn,
        "jj_native_registrations",
        &[
            ("source_id", "TEXT", 1),
            ("baseline_id", "TEXT", 0),
            ("source_root_key", "TEXT", 0),
            ("record", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    );
    assert_columns(
        conn,
        "jj_native_workspaces",
        &[
            ("source_id", "TEXT", 1),
            ("workspace_name", "TEXT", 2),
            ("locator_key", "TEXT", 0),
            ("workspace_root_key", "TEXT", 0),
            ("record", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    );
    let mut registrations = vec![
        index("pk", &["source_id"]),
        index("u", &["source_root_key"]),
    ];
    registrations.sort();
    assert_eq!(index_shapes(conn, "jj_native_registrations"), registrations);
    let mut workspaces = vec![
        index("pk", &["source_id", "workspace_name"]),
        index("u", &["locator_key"]),
        index("u", &["source_id", "workspace_root_key"]),
    ];
    workspaces.sort();
    assert_eq!(index_shapes(conn, "jj_native_workspaces"), workspaces);
    let registration_fk = rows(
        conn,
        "SELECT id, seq, \"table\", \"from\", \"to\", on_update, on_delete, match FROM pragma_foreign_key_list('jj_native_registrations') ORDER BY id, seq",
    );
    let workspace_fk = rows(
        conn,
        "SELECT id, seq, \"table\", \"from\", \"to\", on_update, on_delete, match FROM pragma_foreign_key_list('jj_native_workspaces') ORDER BY id, seq",
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
    assert_eq!(
        registration_fk,
        vec![
            fk(0, "jj_native_baselines", "source_id", "source_id"),
            fk(1, "jj_native_baselines", "baseline_id", "baseline_id")
        ]
    );
    assert_eq!(
        workspace_fk,
        vec![fk(0, "jj_native_registrations", "source_id", "source_id")]
    );
}

pub(super) fn registered_payload_snapshot(conn: &Connection) -> Vec<Vec<Vec<Value>>> {
    let mut result = opaque_snapshot(conn);
    result.extend(native_snapshot(conn));
    for table in ["jj_native_registrations", "jj_native_workspaces"] {
        result.push(rows(conn, &format!("SELECT * FROM {table} ORDER BY rowid")));
    }
    result
}
