use super::*;
use ciborium::Value;
use sha2::{Digest, Sha256};

pub fn heads(anchors: &[JjOperationEvidence]) -> Vec<String> {
    anchors
        .iter()
        .map(|anchor| anchor.operation_id.clone())
        .collect()
}

pub fn canonical_anchors(anchors: &[JjOperationEvidence]) -> Vec<JjOperationEvidence> {
    let mut anchors = anchors.to_vec();
    anchors.sort_by(|a, b| a.operation_id.cmp(&b.operation_id));
    anchors
}

pub fn full_budget() -> ReadBudget {
    ReadBudget::new(8 * 1024 * 1024 + 128 * 1024)
}

pub fn install(
    journal: &mut JjObservationJournal,
    source: &str,
    anchors: &[JjOperationEvidence],
) -> Result<BaselinePersistenceOutcome, JjBaselinePersistenceError> {
    let heads = heads(anchors);
    let prepared =
        prepare_current_state_baseline(JJ_OBSERVATION_READER_PROFILE, &heads, anchors).unwrap();
    persist_current_state_baseline(journal, source, 0, &prepared)
}

pub fn reopen(
    journal: &JjObservationJournal,
    source: &str,
) -> Result<Option<DurableCurrentStateBaseline>, JjBaselinePersistenceError> {
    reopen_current_state_baseline(journal, source, &mut full_budget())
}

pub fn installed(outcome: BaselinePersistenceOutcome) -> BaselineReceipt {
    match outcome {
        BaselinePersistenceOutcome::Installed(receipt) => receipt,
        BaselinePersistenceOutcome::AlreadyInstalled(_) => panic!("expected first installation"),
    }
}

pub fn already(outcome: BaselinePersistenceOutcome) -> BaselineReceipt {
    match outcome {
        BaselinePersistenceOutcome::AlreadyInstalled(receipt) => receipt,
        BaselinePersistenceOutcome::Installed(_) => panic!("expected receipt retry"),
    }
}

pub fn rejected<T>(
    result: Result<T, JjBaselinePersistenceError>,
    category: &str,
) -> JjBaselinePersistenceError {
    let error = match result {
        Ok(_) => panic!("expected {category} rejection"),
        Err(error) => error,
    };
    let text = error.to_string().to_ascii_lowercase();
    assert!(text.contains(category), "expected {category}: {text}");
    error
}

pub fn assert_receipt(receipt: &BaselineReceipt, source: &str, heads: &[String]) {
    assert_eq!(receipt.source_id(), source);
    assert_eq!(receipt.reader_profile(), JJ_OBSERVATION_READER_PROFILE);
    assert_eq!(receipt.boundary(), JjBaselineBoundary::CurrentState);
    assert_eq!(receipt.expected_native_generation(), 0);
    assert_eq!(receipt.generation(), 1);
    assert_eq!(receipt.baseline_id().len(), 64);
    assert!(
        receipt
            .baseline_id()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    let mut expected = heads.to_vec();
    expected.sort();
    assert_eq!(receipt.captured_head_ids(), expected);
}

pub fn assert_same_receipt(actual: &BaselineReceipt, expected: &BaselineReceipt) {
    assert_receipt(actual, expected.source_id(), expected.captured_head_ids());
    assert_eq!(actual.baseline_id(), expected.baseline_id());
}

pub fn native_counts(fixture: &Fixture) -> (usize, usize) {
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let baseline = conn
        .query_row(
            "SELECT COUNT(*) FROM jj_native_baselines WHERE source_id = ?1",
            [&fixture.source],
            |row| row.get(0),
        )
        .unwrap();
    let sources = conn
        .query_row(
            "SELECT COUNT(*) FROM jj_native_sources WHERE source_id = ?1",
            [&fixture.source],
            |row| row.get(0),
        )
        .unwrap();
    (baseline, sources)
}

pub fn stored_bytes(fixture: &Fixture) -> usize {
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.query_row("SELECT length(state) + length(record) FROM jj_native_sources s JOIN jj_native_baselines b USING (source_id, baseline_id) WHERE s.source_id = ?1", [&fixture.source], |row| row.get(0)).unwrap()
}

pub fn native_rows(fixture: &Fixture) -> Vec<(String, String, Vec<u8>, String)> {
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let mut statement = conn.prepare("SELECT 'baseline', baseline_id, record, checksum FROM jj_native_baselines WHERE source_id = ?1 UNION ALL SELECT 'state', baseline_id, state, checksum FROM jj_native_sources WHERE source_id = ?1 ORDER BY 1, 2").unwrap();
    statement
        .query_map([&fixture.source], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn encode(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).unwrap();
    bytes
}

pub fn field<'a>(value: &'a mut Value, name: &str) -> &'a mut Value {
    let Value::Map(entries) = value else {
        panic!("expected record map")
    };
    entries
        .iter_mut()
        .find(|(key, _)| key.as_text() == Some(name))
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("missing {name} field"))
}

pub fn mutate_state(fixture: &Fixture, mutation: impl FnOnce(&mut Value)) {
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let raw: Vec<u8> = conn
        .query_row(
            "SELECT state FROM jj_native_sources WHERE source_id = ?1",
            [&fixture.source],
            |row| row.get(0),
        )
        .unwrap();
    let mut state: Value = ciborium::from_reader(raw.as_slice()).unwrap();
    mutation(&mut state);
    let raw = encode(&state);
    conn.execute(
        "UPDATE jj_native_sources SET state = ?1, checksum = ?2 WHERE source_id = ?3",
        params![raw, hash(&raw), fixture.source],
    )
    .unwrap();
}

/// Repairs every storage hash and pointer so a native fault reaches the decoder.
pub fn mutate_record_and_rekey(fixture: &Fixture, mutation: impl FnOnce(&mut Value)) {
    let mut conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    let raw: Vec<u8> = conn
        .query_row(
            "SELECT record FROM jj_native_baselines WHERE source_id = ?1",
            [&fixture.source],
            |row| row.get(0),
        )
        .unwrap();
    let mut record: Value = ciborium::from_reader(raw.as_slice()).unwrap();
    mutation(&mut record);
    let raw = encode(&record);
    let id = hash(&raw);
    let state_raw: Vec<u8> = conn
        .query_row(
            "SELECT state FROM jj_native_sources WHERE source_id = ?1",
            [&fixture.source],
            |row| row.get(0),
        )
        .unwrap();
    let mut state: Value = ciborium::from_reader(state_raw.as_slice()).unwrap();
    *field(&mut state, "baseline_id") = Value::Text(id.clone());
    let state_raw = encode(&state);
    let tx = conn.transaction().unwrap();
    tx.execute("UPDATE jj_native_baselines SET baseline_id = ?1, record = ?2, checksum = ?3 WHERE source_id = ?4", params![id, raw, hash(&raw), fixture.source]).unwrap();
    tx.execute("UPDATE jj_native_sources SET baseline_id = ?1, state = ?2, checksum = ?3 WHERE source_id = ?4", params![id, state_raw, hash(&state_raw), fixture.source]).unwrap();
    tx.commit().unwrap();
}
