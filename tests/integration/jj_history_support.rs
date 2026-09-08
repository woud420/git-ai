use super::*;
use git_ai::model::jj_observation::{JJ_OBSERVATION_SCHEMA_VERSION, JjObservationBatch};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn history_fixture_case(name: &str, fixture: &mut Fixture, policy: Policy) {
    run_fixture_case(
        name,
        fixture,
        policy,
        "jj_capture::registration::registration_case_child",
    );
}

fn home_files(case: &Case) -> BTreeMap<PathBuf, Entry> {
    let mut files = manifest(&case.test_home);
    for name in [
        "registration.sqlite",
        "registration.sqlite-wal",
        "registration.sqlite-shm",
        "registration-child.stdout",
        "registration-child.stderr",
    ] {
        files.remove(&PathBuf::from(name));
    }
    files
}

fn schema(case: &Case) -> Vec<(String, String, String, Option<String>)> {
    let conn = case.sql();
    let mut query = conn
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name LIMIT 129",
        )
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

pub fn checked(
    case: &Case,
    journal: &JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    until: Instant,
    read_budget: &mut ReadBudget,
) -> Result<CollectedJjHistory, JjHistoryCollectionError> {
    let before = state(case);
    let home = home_files(case);
    let old_schema = schema(case);
    let result = collect_registered_history(journal, context, config, until, read_budget);
    assert!(
        state(case) == before,
        "collector changed repository or SQL state"
    );
    assert!(
        home_files(case) == home,
        "collector changed fixture home files"
    );
    assert!(schema(case) == old_schema, "collector changed SQL schema");
    result
}

pub fn collect(case: &Case, journal: &JjObservationJournal, config: &Config) -> CollectedJjHistory {
    checked(
        case,
        journal,
        &case.context(),
        config,
        deadline(),
        &mut budget(),
    )
    .unwrap()
}

pub fn rejected(
    case: &Case,
    journal: &JjObservationJournal,
    config: &Config,
) -> JjHistoryCollectionError {
    failure(checked(
        case,
        journal,
        &case.context(),
        config,
        deadline(),
        &mut budget(),
    ))
}

pub fn failure<T>(result: Result<T, JjHistoryCollectionError>) -> JjHistoryCollectionError {
    match result {
        Ok(_) => panic!("incomplete or unavailable source produced history"),
        Err(error) => {
            let standard: &dyn std::error::Error = &error;
            assert!(!standard.to_string().is_empty());
            error
        }
    }
}

pub fn install(case: &Case, config: &Config) -> (JjObservationJournal, RegisteredJjCurrentState) {
    let mut journal = case.open();
    let registered = installed(case.register(&mut journal, config).unwrap());
    drop(journal);
    (case.open(), registered)
}

pub fn assert_result(
    result: &CollectedJjHistory,
    registered: &RegisteredJjCurrentState,
    heads: &[&str],
    records: &[JjOperationEvidence],
    reached: &[&str],
    root: bool,
) {
    super::super::behavior::same_receipt(result.registration(), registered);
    let mut heads = heads.to_vec();
    heads.sort();
    assert_eq!(result.head_ids(), heads);
    assert_eq!(result.ordered_operations(), records);
    let mut reached = reached.to_vec();
    reached.sort();
    assert_eq!(result.reached_baseline_ids(), reached);
    assert_eq!(result.reaches_root(), root);
    for record in result.ordered_operations() {
        verify_evidence(JJ_OBSERVATION_READER_PROFILE, record).unwrap();
    }
}

pub fn write_records(case: &Case, records: &[JjOperationEvidence]) {
    for record in records {
        verify_evidence(JJ_OBSERVATION_READER_PROFILE, record).unwrap();
        case.write_evidence(record);
    }
}

pub fn op_path(case: &Case, id: &str) -> PathBuf {
    case.repo_dir.join("op_store/operations").join(id)
}

pub fn view_path(case: &Case, id: &str) -> PathBuf {
    case.repo_dir.join("op_store/views").join(id)
}

pub fn opaque(case: &Case, journal: &mut JjObservationJournal, source: &str) {
    let records = vec![first(), left()];
    journal
        .capture(&JjObservationBatch {
            schema_version: JJ_OBSERVATION_SCHEMA_VERSION,
            reader_profile: JJ_OBSERVATION_READER_PROFILE.to_owned(),
            source_id: source.to_owned(),
            expected_generation: 0,
            expected_observed_heads: vec![],
            captured_integrated_heads: vec![LEFT_ID.to_owned()],
            operations: records,
        })
        .unwrap();
    let observed = journal
        .lookup_observed(source, &[LEFT_ID.to_owned()])
        .unwrap();
    assert_eq!(observed.status.generation, 1);
    assert_eq!(observed.operations[LEFT_ID], left());
    assert!(!op_path(case, LEFT_ID).exists());
}
