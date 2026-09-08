use super::*;

pub(super) const DDL: &str = "CREATE TABLE jj_observer_intent (slot INTEGER PRIMARY KEY CHECK (slot = 1), revision INTEGER NOT NULL CHECK (revision > 0), payload BLOB NOT NULL CHECK (length(payload) <= 524288))";
pub(super) const MAX_PAYLOAD: usize = 512 * 1024;

pub(super) struct Fixture {
    pub(super) directory: tempfile::TempDir,
    pub(super) path: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("observer.db");
        Self { directory, path }
    }

    pub(super) fn manual(&self, ddl: &str, version: i64, wal: bool) -> Connection {
        let connection = sqlite::open_with_memory_limits(&self.path).unwrap();
        if wal {
            let mode: String = connection
                .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
        }
        connection.execute_batch(ddl).unwrap();
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        connection
    }

    pub(super) fn connection(&self) -> Connection {
        sqlite::open_with_flags_and_memory_limits(&self.path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .unwrap()
    }
}

pub(super) fn record(revision: u64) -> StoredIntent {
    StoredIntent {
        schema_version: 1,
        revision,
        target: StoredTarget {
            journal_path_hex: hex(b"/fixture/native-journal.db"),
            workspace_path_hex: hex(b"/fixture/workspace"),
            metadata: JjObserverTarget {
                source_id: "0".repeat(64),
                initialization_receipt_id: "1".repeat(64),
                reader_profile: JJ_OBSERVATION_READER_PROFILE.to_owned(),
                baseline_id: "2".repeat(64),
                baseline_generation: 1,
                workspace_name: "workspace-工".to_owned(),
                attachment_id: "3".repeat(64),
            },
        },
        enabled: true,
        blocked: None,
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn value(record: &StoredIntent) -> Value {
    serde_json::to_value(record).unwrap()
}

pub(super) fn encoded(record: &StoredIntent) -> Vec<u8> {
    serde_json::to_vec(record).unwrap()
}

pub(super) fn raw_insert(connection: &Connection, revision: i64, raw: &[u8]) {
    connection
        .execute(
            "INSERT INTO jj_observer_intent(slot,revision,payload) VALUES(1,?1,?2)",
            params![revision, raw],
        )
        .unwrap();
}

pub(super) fn assert_loaded(path: &Path, expected: &StoredIntent) {
    let loaded = load(path).unwrap().expect("saved intent");
    assert_eq!(value(&loaded), value(expected));
}

#[derive(Debug, PartialEq)]
pub(super) struct Snapshot {
    schema: Vec<Vec<rusqlite::types::Value>>,
    rows: Vec<Vec<rusqlite::types::Value>>,
    version: i64,
    mode: String,
}

fn query(connection: &Connection, sql: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let mut statement = connection.prepare(sql).unwrap();
    let count = statement.column_count();
    statement
        .query_map([], |row| {
            (0..count)
                .map(|column| row.get(column))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

pub(super) fn snapshot(connection: &Connection) -> Snapshot {
    Snapshot {
        schema: query(
            connection,
            "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name",
        ),
        rows: query(
            connection,
            "SELECT * FROM jj_observer_intent ORDER BY rowid",
        ),
        version: connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap(),
        mode: connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap(),
    }
}

pub(super) fn assert_validation(error: ObserverStoreError) {
    assert!(
        matches!(error, ObserverStoreError::Validation(_)),
        "{error}"
    );
}

pub(super) fn refuse_raw(raw: &[u8], revision: i64) {
    let fixture = Fixture::new();
    let connection = fixture.manual(DDL, 1, true);
    connection
        .pragma_update(None, "ignore_check_constraints", true)
        .unwrap();
    raw_insert(&connection, revision, raw);
    let before = snapshot(&connection);
    assert_validation(load(&fixture.path).unwrap_err());
    assert!(replace(&fixture.path, &Some(record(1)), &record(2)).is_err());
    assert_eq!(snapshot(&connection), before);
}
