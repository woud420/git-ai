use super::*;

pub(super) const RECORD_LIMIT: usize = 128 * 1024;

pub(super) fn vector(label: &str) -> &'static RecordVector {
    RECORDS.iter().find(|record| record.label == label).unwrap()
}

pub(super) fn record_fixture() -> (Fixture, Connection) {
    let fixture = manual_v3(REGISTRATIONS, WORKSPACES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    (fixture, conn)
}

pub(super) fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

pub(super) fn table(record: &RecordVector) -> &'static str {
    match record.kind {
        "source" => "jj_native_registrations",
        "workspace" => "jj_native_workspaces",
        other => panic!("unexpected fixture kind {other}"),
    }
}

pub(super) fn insert_record(conn: &Connection, record: &RecordVector) {
    match record.kind {
        "source" => {
            conn.execute(
                "INSERT INTO jj_native_registrations
                 (source_id, baseline_id, source_root_key, record, checksum)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    record.source_id,
                    record.baseline_id,
                    record.source_root_key,
                    record.raw,
                    record.checksum
                ],
            )
            .unwrap();
        }
        "workspace" => {
            conn.execute(
                "INSERT INTO jj_native_workspaces
                 (source_id, workspace_name, locator_key, workspace_root_key, record, checksum)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    record.source_id,
                    record.workspace_name,
                    record.locator_key,
                    record.workspace_root_key,
                    record.raw,
                    record.checksum
                ],
            )
            .unwrap();
        }
        other => panic!("unexpected fixture kind {other}"),
    }
}

pub(super) fn reset_record(conn: &Connection, record: &RecordVector) {
    conn.execute("DELETE FROM jj_native_workspaces", [])
        .unwrap();
    conn.execute("DELETE FROM jj_native_registrations", [])
        .unwrap();
    insert_record(conn, record);
}

pub(super) fn read_record(
    journal: &JjObservationJournal,
    record: &RecordVector,
    budget: &mut ReadBudget,
) -> Result<Option<Vec<u8>>, JournalError> {
    match record.kind {
        "source" => journal.read_native_registration_record(record.source_id, budget),
        "workspace" => {
            journal.read_native_workspace_record(record.source_id, record.workspace_name, budget)
        }
        other => panic!("unexpected fixture kind {other}"),
    }
}

pub(super) fn checked_read(
    journal: &JjObservationJournal,
    conn: &Connection,
    record: &RecordVector,
    budget: &mut ReadBudget,
) -> Result<Option<Vec<u8>>, JournalError> {
    let before = complete_snapshot(conn);
    let result = read_record(journal, record, budget);
    assert_eq!(
        complete_snapshot(conn),
        before,
        "{} mutated storage",
        record.label
    );
    result
}

pub(super) fn replace_raw(conn: &Connection, record: &RecordVector, raw: &[u8]) {
    conn.execute(
        &format!("UPDATE {} SET record = ?1, checksum = ?2", table(record)),
        params![raw, digest(raw)],
    )
    .unwrap();
}

pub(super) fn change_text(raw: &[u8], field: &str, replacement: &str) -> Vec<u8> {
    let mut value: Cbor = ciborium::from_reader(raw).unwrap();
    let Cbor::Map(entries) = &mut value else {
        panic!("fixture is not a map")
    };
    let entry = entries
        .iter_mut()
        .find(|(key, _)| key.as_text() == Some(field))
        .unwrap();
    entry.1 = Cbor::Text(replacement.to_owned());
    let mut changed = Vec::new();
    ciborium::into_writer(&value, &mut changed).unwrap();
    changed
}

pub(super) fn unconstrain_after_open(conn: &Connection, record: &RecordVector) {
    let table = table(record);
    conn.execute_batch(&format!(
        "CREATE TABLE record_schema_copy AS SELECT * FROM {table};
         DROP TABLE {table};
         ALTER TABLE record_schema_copy RENAME TO {table};"
    ))
    .unwrap();
}

pub(super) fn assert_charged_error(
    journal: &JjObservationJournal,
    conn: &Connection,
    record: &RecordVector,
    bytes: usize,
) {
    let mut budget = ReadBudget::new(RECORD_LIMIT * 2);
    assert!(
        checked_read(journal, conn, record, &mut budget).is_err(),
        "{}",
        record.label
    );
    assert_eq!(budget.consumed(), bytes, "{}", record.label);
}
