use super::*;
use rusqlite::types::Value as SqlValue;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const ADMISSION_TABLES: [&str; 2] = ["jj_native_admissions", "jj_native_admission_states"];

pub fn admission_budget() -> ReadBudget {
    // Three complete registration selections plus two independently capped packets.
    ReadBudget::new(48 * 1024 * 1024)
}

pub fn admission_rows(case: &Case) -> Vec<(String, Vec<Vec<SqlValue>>)> {
    let conn = case.sql();
    ADMISSION_TABLES
        .into_iter()
        .map(|table| {
            let mut query = conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1, 2 LIMIT 17"))
                .unwrap();
            let columns = query.column_count();
            let rows = query
                .query_map([], |row| {
                    (0..columns)
                        .map(|i| row.get(i))
                        .collect::<rusqlite::Result<Vec<SqlValue>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            assert!(rows.len() < 17);
            (table.to_owned(), rows)
        })
        .collect()
}

fn home(case: &Case) -> BTreeMap<PathBuf, Entry> {
    crate::jj_capture::support::manifest_excluding(
        &case.test_home,
        &[
            "registration.sqlite",
            "registration.sqlite-wal",
            "registration.sqlite-shm",
            "registration-child.stdout",
            "registration-child.stderr",
        ],
    )
}

fn schema(case: &Case) -> Vec<(String, String, String, Option<String>)> {
    let conn = case.sql();
    let mut query = conn
        .prepare("SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name LIMIT 129")
        .unwrap();
    let rows = query
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(rows.len() < 129);
    rows
}

#[derive(PartialEq)]
pub struct TotalState {
    old: State,
    home: BTreeMap<PathBuf, Entry>,
    schema: Vec<(String, String, String, Option<String>)>,
    admissions: Vec<(String, Vec<Vec<SqlValue>>)>,
}

pub fn total_state(case: &Case) -> TotalState {
    TotalState {
        old: state(case),
        home: home(case),
        schema: schema(case),
        admissions: admission_rows(case),
    }
}

pub fn failed<T>(result: Result<T, JjNativeAdmissionError>) -> JjNativeAdmissionError {
    match result {
        Ok(_) => panic!("invalid admission input or durable state was accepted"),
        Err(error) => {
            let standard: &dyn std::error::Error = &error;
            assert!(!standard.to_string().is_empty());
            error
        }
    }
}

pub fn checked_status(
    case: &Case,
    journal: &JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    until: Instant,
    reads: &mut ReadBudget,
) -> Result<RegisteredNativeAdmissionState, JjNativeAdmissionError> {
    let before = total_state(case);
    let result = read_registered_admission_state(journal, context, config, until, reads);
    assert!(
        total_state(case) == before,
        "admission status changed fixture state"
    );
    result
}

pub fn checked_known(
    case: &Case,
    journal: &JjObservationJournal,
    source: &str,
    id: &str,
    until: Instant,
    reads: &mut ReadBudget,
) -> Result<Option<DurableNativeAdmission>, JjNativeAdmissionError> {
    let before = total_state(case);
    let result = read_native_admission(journal, source, id, until, reads);
    assert!(
        total_state(case) == before,
        "historical admission read changed fixture state"
    );
    result
}

pub fn admit_checked(
    case: &Case,
    journal: &mut JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    expected: NativeAdmissionExpectation<'_>,
    until: Instant,
    reads: &mut ReadBudget,
) -> Result<NativeAdmissionOutcome, JjNativeAdmissionError> {
    let before = total_state(case);
    let result = admit_registered_history(journal, context, config, expected, until, reads);
    let after = total_state(case);
    assert!(
        after.old == before.old,
        "admission changed repository, registration or opaque state"
    );
    assert!(
        after.home == before.home,
        "admission changed fixture home files"
    );
    assert!(
        after.schema == before.schema,
        "admission changed SQL schema"
    );
    if result.is_err() {
        assert!(
            after.admissions == before.admissions,
            "failed admission partially changed its rows"
        );
    }
    result
}

pub fn status(
    case: &Case,
    journal: &JjObservationJournal,
    config: &Config,
) -> RegisteredNativeAdmissionState {
    checked_status(
        case,
        journal,
        &case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap()
}

pub fn initial(
    case: &Case,
    config: &Config,
) -> (
    JjObservationJournal,
    RegisteredJjCurrentState,
    NativeAdmissionCursor,
) {
    let (journal, registered) = install(case, config);
    let observed = status(case, &journal, config);
    same_receipt(observed.registration(), &registered);
    assert!(observed.latest_receipt().is_none());
    assert_cursor(observed.cursor(), &registered, 0, &[MERGE_ID]);
    (journal, registered, observed.cursor().clone())
}

pub fn admit_now(
    case: &Case,
    journal: &mut JjObservationJournal,
    config: &Config,
    expected: &NativeAdmissionCursor,
) -> NativeAdmissionOutcome {
    admit_checked(
        case,
        journal,
        &case.context(),
        config,
        expected.expectation(),
        deadline(),
        &mut admission_budget(),
    )
    .unwrap()
}

pub fn outcome(result: NativeAdmissionOutcome, already: bool) -> RegisteredNativeAdmission {
    match (result, already) {
        (NativeAdmissionOutcome::Admitted(value), false)
        | (NativeAdmissionOutcome::AlreadyAdmitted(value), true) => value,
        _ => panic!("unexpected admission disposition"),
    }
}

pub fn packet(
    case: &Case,
    journal: &JjObservationJournal,
    source: &str,
    id: &str,
) -> DurableNativeAdmission {
    checked_known(
        case,
        journal,
        source,
        id,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap()
    .unwrap()
}

pub fn select(
    case: &Case,
    records: &[JjOperationEvidence],
    heads: &[&str],
    checkout: Option<&str>,
) {
    write_records(case, records);
    case.heads(heads);
    if let Some(id) = checkout {
        case.write_checkout(id);
    }
    capture_current_state(&case.context(), deadline()).unwrap();
}

pub fn assert_cursor(
    cursor: &NativeAdmissionCursor,
    saved: &RegisteredJjCurrentState,
    generation: u64,
    heads: &[&str],
) {
    assert_eq!(cursor.source_id(), saved.source_id());
    assert_eq!(
        cursor.initialization_receipt_id(),
        saved.initialization_receipt_id()
    );
    assert_eq!(cursor.reader_profile(), JJ_OBSERVATION_READER_PROFILE);
    assert_eq!(
        cursor.baseline_id(),
        saved.baseline().receipt().baseline_id()
    );
    assert_eq!(cursor.baseline_generation(), 1);
    assert_eq!(cursor.generation(), generation);
    let mut heads = heads.to_vec();
    heads.sort();
    assert_eq!(cursor.admitted_head_ids(), heads);
    let expected = cursor.expectation();
    assert_eq!(expected.source_id, cursor.source_id());
    assert_eq!(
        expected.initialization_receipt_id,
        cursor.initialization_receipt_id()
    );
    assert_eq!(expected.baseline_id, cursor.baseline_id());
    assert_eq!(expected.generation, cursor.generation());
    assert_eq!(expected.admitted_head_ids, cursor.admitted_head_ids());
}

pub fn assert_admission(
    case: &Case,
    actual: &DurableNativeAdmission,
    saved: &RegisteredJjCurrentState,
    expected: &NativeAdmissionCursor,
    heads: &[&str],
    records: &[JjOperationEvidence],
    boundary: (&[&str], bool),
) {
    let (reached, root) = boundary;
    let receipt: &NativeAdmissionReceipt = actual.receipt();
    hex_id(receipt.admission_id());
    assert_eq!(receipt.source_id(), saved.source_id());
    assert_eq!(
        receipt.initialization_receipt_id(),
        saved.initialization_receipt_id()
    );
    assert_eq!(receipt.reader_profile(), JJ_OBSERVATION_READER_PROFILE);
    assert_eq!(
        receipt.baseline_id(),
        saved.baseline().receipt().baseline_id()
    );
    assert_eq!(receipt.baseline_generation(), 1);
    assert_eq!(receipt.expected_generation(), expected.generation());
    assert_eq!(
        receipt.expected_admitted_head_ids(),
        expected.admitted_head_ids()
    );
    assert_eq!(receipt.generation(), expected.generation() + 1);
    let mut heads = heads.to_vec();
    heads.sort();
    let mut reached = reached.to_vec();
    reached.sort();
    assert_eq!(receipt.captured_head_ids(), heads);
    assert_eq!(actual.ordered_operations(), records);
    assert_eq!(actual.reached_baseline_ids(), reached);
    assert_eq!(actual.reaches_root(), root);
    for record in actual.ordered_operations() {
        verify_evidence(JJ_OBSERVATION_READER_PROFILE, record).unwrap();
    }
    let (raw, checksum, generation): (Vec<u8>, String, u64) = case.sql().query_row(
        "SELECT record,checksum,generation FROM jj_native_admissions WHERE source_id COLLATE BINARY=?1 AND admission_id COLLATE BINARY=?2",
        [saved.source_id(),receipt.admission_id()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(hash(&raw), receipt.admission_id());
    assert_eq!(checksum, receipt.admission_id());
    assert_eq!(generation, receipt.generation());
}
