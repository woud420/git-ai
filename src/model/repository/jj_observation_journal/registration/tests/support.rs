use super::*;
use std::path::PathBuf;

pub(super) const SOURCE: &str = "0000000000000000000000000000000000000000000000000000000000000001";
pub(super) const LIMIT: usize = 8 * 1024 * 1024 + 4 * 128 * 1024;
pub(super) const SOURCE_RAW: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/jj-registration-records/linux_source.cbor"
));
pub(super) const WORKSPACE_RAW: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/jj-registration-records/linux_workspace.cbor"
));
const V2: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/jj-observation-v2.sql"
));

pub(super) struct Fixture {
    _home: tempfile::TempDir,
    pub path: PathBuf,
    pub journal: JjObservationJournal,
    pub seed: NativeBaselineSnapshot,
}

impl Fixture {
    pub fn new(keep_native: bool) -> Self {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("registration.sqlite");
        open_with_memory_limits(&path)
            .unwrap()
            .execute_batch(V2)
            .unwrap();
        let journal = JjObservationJournal::open_at_path(&path).unwrap();
        let seed = journal
            .read_native_baseline(SOURCE, &mut ReadBudget::new(LIMIT))
            .unwrap()
            .unwrap();
        if !keep_native {
            journal
                .conn
                .execute_batch("DELETE FROM jj_native_sources; DELETE FROM jj_native_baselines;")
                .unwrap();
        }
        Self {
            _home: home,
            path,
            journal,
            seed,
        }
    }

    pub fn complete() -> Self {
        let fixture = Self::new(true);
        insert_source(&fixture.journal.conn, SOURCE_RAW);
        insert_workspace(&fixture.journal.conn, WORKSPACE_RAW);
        fixture
    }

    pub fn independent(&self) -> Connection {
        open_with_memory_limits(&self.path).unwrap()
    }
}

pub(super) fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

pub(super) fn encode(record: &impl serde::Serialize) -> Vec<u8> {
    let mut raw = Vec::new();
    ciborium::into_writer(record, &mut raw).unwrap();
    raw
}

pub(super) fn source_record() -> RegistrationRecord {
    ciborium::from_reader(SOURCE_RAW).unwrap()
}

pub(super) fn workspace_record() -> WorkspaceRecord {
    ciborium::from_reader(WORKSPACE_RAW).unwrap()
}

pub(super) fn metadata() -> RegistrationMetadata {
    let source = source_record();
    let workspace = workspace_record();
    RegistrationMetadata {
        seal_bytes: source.seal_bytes.0,
        source_binding: source.source_binding,
        workspace: WorkspaceRegistrationMetadata {
            workspace_name: workspace.workspace_name,
            attachment_id: workspace.attachment_id,
            locator: workspace.locator,
            workspace_binding: workspace.workspace_binding,
            selected_checkout: workspace.selected_checkout,
        },
    }
}

pub(super) fn prepared<'a>(seed: &'a NativeBaselineSnapshot) -> PreparedRegistrationInstall<'a> {
    let anchors: Vec<_> = seed.record.anchors.iter().collect();
    PreparedRegistrationInstall::new(
        SOURCE,
        &seed.record.reader_profile,
        &seed.record.captured_head_ids,
        &anchors,
        metadata(),
    )
    .unwrap()
}

pub(super) fn insert_source(conn: &Connection, raw: &[u8]) {
    let record: RegistrationRecord = ciborium::from_reader(raw).unwrap();
    conn.execute(
        "INSERT INTO jj_native_registrations(source_id, baseline_id, source_root_key, record, checksum)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![record.source_id, record.baseline_id, source_root_guard(&record.source_binding).unwrap(), raw, digest(raw)],
    ).unwrap();
}

pub(super) fn insert_workspace(conn: &Connection, raw: &[u8]) {
    let record: WorkspaceRecord = ciborium::from_reader(raw).unwrap();
    conn.execute(
        "INSERT INTO jj_native_workspaces(source_id, workspace_name, locator_key, workspace_root_key, record, checksum)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![record.source_id, record.workspace_name, workspace_locator_guard(&record.locator).unwrap(),
            workspace_root_guard(&record.locator, &record.workspace_binding).unwrap(), raw, digest(raw)],
    ).unwrap();
}

pub(super) fn replace_record(conn: &Connection, table: &str, raw: &[u8]) {
    conn.execute(
        &format!("UPDATE {table} SET record=?1, checksum=?2"),
        params![raw, digest(raw)],
    )
    .unwrap();
}

pub(super) fn native_counts(conn: &Connection) -> [u64; 4] {
    [
        "jj_native_baselines",
        "jj_native_sources",
        "jj_native_registrations",
        "jj_native_workspaces",
    ]
    .map(|table| {
        conn.query_row(
            &format!("SELECT count(*) FROM {table} WHERE source_id=?1"),
            [SOURCE],
            |row| row.get(0),
        )
        .unwrap()
    })
}

pub(super) fn snapshot(conn: &Connection) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    [
        "jj_sources",
        "jj_operations",
        "jj_batches",
        "jj_views",
        "jj_native_baselines",
        "jj_native_sources",
        "jj_native_registrations",
        "jj_native_workspaces",
    ]
    .iter()
    .map(|table| {
        let mut statement = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns).map(|column| row.get(column)).collect()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    })
    .collect()
}

pub(super) fn charged_lengths(conn: &Connection) -> [usize; 4] {
    [
        ("jj_native_registrations", "record"),
        ("jj_native_workspaces", "record"),
        ("jj_native_sources", "state"),
        ("jj_native_baselines", "record"),
    ]
    .map(|(table, field)| {
        conn.query_row(
            &format!("SELECT length({field}) FROM {table} WHERE source_id=?1"),
            [SOURCE],
            |row| row.get(0),
        )
        .unwrap()
    })
}

pub(super) fn error_text<T>(result: Result<T, JournalError>) -> String {
    result.err().expect("expected a storage error").to_string()
}

pub(super) fn read(
    fixture: &Fixture,
    name: &str,
    budget: &mut ReadBudget,
) -> Result<Option<StoredRegistrationSnapshot>, JournalError> {
    let before = snapshot(&fixture.journal.conn);
    let result = fixture
        .journal
        .read_registration_snapshot(SOURCE, name, budget);
    assert_eq!(snapshot(&fixture.journal.conn), before);
    result
}
