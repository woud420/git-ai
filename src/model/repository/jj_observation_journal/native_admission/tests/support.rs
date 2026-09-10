use super::*;
use crate::model::repository::sqlite::open_with_memory_limits;
use rusqlite::{Connection, params, types::Value as SqlValue};
use std::path::PathBuf;

pub(super) struct Fixture {
    _home: tempfile::TempDir,
    pub path: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("admission.sqlite");
        let conn = open_with_memory_limits(&path).unwrap();
        conn.execute_batch(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/jj-observation-v3.sql"
        )))
        .unwrap();
        drop(conn);
        drop(JjObservationJournal::open_at_path(&path).unwrap());
        Self { _home: home, path }
    }
    pub fn journal(&self) -> JjObservationJournal {
        JjObservationJournal::open_at_path(&self.path).unwrap()
    }
    pub fn conn(&self) -> Connection {
        open_with_memory_limits(&self.path).unwrap()
    }
    pub fn insert(&self, label: &str) {
        let item = vector(label);
        let conn = self.conn();
        if item.kind == "packet" {
            conn.execute(
                "INSERT INTO jj_native_admissions VALUES (?1,?2,?3,?4,?5)",
                params![
                    vectors::SOURCE,
                    item.admission_id,
                    item.generation,
                    item.raw,
                    item.checksum
                ],
            )
            .unwrap();
        } else {
            conn.execute(
                "INSERT INTO jj_native_admission_states VALUES (?1,?2,?3,?4)",
                params![vectors::SOURCE, item.admission_id, item.raw, item.checksum],
            )
            .unwrap();
        }
    }
    pub fn seed(&self, latest: &str) {
        if latest != "left" {
            self.insert("left");
        }
        self.insert(latest);
        self.insert(&format!("{latest}_state"));
    }
    pub fn registration_bytes(&self) -> usize {
        let conn = self.conn();
        [
            "SELECT length(record) FROM jj_native_registrations",
            "SELECT length(record) FROM jj_native_workspaces",
            "SELECT length(record) FROM jj_native_baselines",
            "SELECT length(state) FROM jj_native_sources",
        ]
        .iter()
        .map(|sql| {
            conn.query_row(sql, [], |row| row.get::<_, usize>(0))
                .unwrap()
        })
        .sum()
    }
    pub fn packet_rows(&self) -> Vec<Vec<SqlValue>> {
        rows(
            &self.conn(),
            "SELECT * FROM jj_native_admissions ORDER BY source_id,generation,admission_id",
        )
    }
    pub fn state_rows(&self) -> Vec<Vec<SqlValue>> {
        rows(
            &self.conn(),
            "SELECT * FROM jj_native_admission_states ORDER BY source_id",
        )
    }
    pub fn old_rows(&self) -> Vec<Vec<Vec<SqlValue>>> {
        let conn = self.conn();
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
        .map(|table| rows(&conn, &format!("SELECT * FROM {table} ORDER BY rowid")))
        .collect()
    }
}
pub(super) fn rows(conn: &Connection, sql: &str) -> Vec<Vec<SqlValue>> {
    let mut statement = conn.prepare(sql).unwrap();
    let count = statement.column_count();
    statement
        .query_map([], |row| {
            (0..count)
                .map(|index| row.get(index))
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}
pub(super) fn read(
    journal: &JjObservationJournal,
    requested: Option<&str>,
    budget: &mut ReadBudget,
) -> Result<StoredAdmissionSnapshot, JournalError> {
    journal.read_native_admission_snapshot(vectors::SOURCE, Some("default"), requested, budget)
}
pub(super) fn unlimited() -> ReadBudget {
    ReadBudget::new(64 * 1024 * 1024)
}
pub(super) fn assert_cursor(snapshot: &StoredAdmissionSnapshot, generation: u64, label: &str) {
    assert_eq!(snapshot.cursor.generation, generation);
    assert_eq!(snapshot.cursor.admitted_head_ids, Input::from(label).heads);
}
pub(super) fn committed(fixture: &Fixture, label: &str) {
    let input = Input::from(label);
    let prepared = input.prepare().unwrap();
    let id = prepared.admission_id().to_owned();
    let mut journal = fixture.journal();
    let transaction = journal
        .begin_native_admission(
            vectors::SOURCE,
            Some("default"),
            Some(&id),
            &mut unlimited(),
        )
        .unwrap();
    let staged = transaction.stage(prepared, &mut unlimited()).unwrap();
    let (commit, outcome, snapshot) = staged.into_parts();
    assert_eq!(outcome, NativeAdmissionOutcome::Admitted);
    assert_eq!(snapshot.requested().unwrap().admission_id, id);
    commit.commit().unwrap();
}
