use super::*;
use ciborium::Value;
use rusqlite::{ErrorCode, params};
use sha2::{Digest, Sha256};

pub(super) const UNCHANGED_PHASES: [AdmissionPhase; 2] = [
    AdmissionPhase::UnchangedCandidate,
    AdmissionPhase::UnchangedSnapshotVerified,
];

pub(super) fn setup(
    with_prior: bool,
) -> (
    Case,
    JjObservationJournal,
    NativeAdmissionCursor,
    Option<String>,
) {
    let (case, mut journal, zero) = Case::new();
    if !with_prior {
        return (case, journal, zero, None);
    }
    let record = fixtures::rich_parent();
    case.select(std::slice::from_ref(&record), &record);
    let first = admitted(run(&case, &mut journal, &zero, &mut Hook::new(|_| {})).unwrap());
    let expected = first.current_cursor().clone();
    let latest = first.admission().receipt().admission_id().to_owned();
    (case, journal, expected, Some(latest))
}

pub(super) fn reconcile(
    case: &Case,
    journal: &mut JjObservationJournal,
    expected: &NativeAdmissionCursor,
    hook: &mut impl AdmissionHooks,
) -> Result<NativeReconciliationOutcome, JjNativeAdmissionError> {
    reconcile_with_hooks(
        journal,
        &case.fixture.context(),
        &case.config,
        NativeReconciliationExpectation {
            admission: expected.expectation(),
            workspace_name: case.registered.workspace_name(),
            attachment_id: case.registered.attachment_id(),
        },
        deadline(),
        &mut reads(),
        hook,
    )
}

pub(super) fn unchanged(outcome: NativeReconciliationOutcome) -> RegisteredNativeAdmissionState {
    match outcome {
        NativeReconciliationOutcome::Unchanged(value) => value,
        NativeReconciliationOutcome::Admission(_) => panic!("matching sampled heads were staged"),
    }
}

pub(super) fn status(case: &Case) -> RegisteredNativeAdmissionState {
    let journal = JjObservationJournal::open_at_path(&case.db).unwrap();
    read_registered_admission_state(
        &journal,
        &case.fixture.context(),
        &case.config,
        deadline(),
        &mut reads(),
    )
    .unwrap()
}

pub(super) fn writer_reservation(path: &Path, blocked: bool) {
    let connection = open_with_memory_limits(path).unwrap();
    connection.busy_timeout(Duration::ZERO).unwrap();
    let result = connection.execute_batch("BEGIN IMMEDIATE");
    if blocked {
        assert_eq!(
            result.unwrap_err().sqlite_error_code(),
            Some(ErrorCode::DatabaseBusy)
        );
    } else {
        result.unwrap();
        connection.execute_batch("ROLLBACK").unwrap();
    }
}

pub(super) fn deny_native_writes(path: &Path) {
    let connection = open_with_memory_limits(path).unwrap();
    for table in [
        "jj_native_baselines",
        "jj_native_sources",
        "jj_native_registrations",
        "jj_native_workspaces",
        "jj_native_admissions",
        "jj_native_admission_states",
    ] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            connection
                .execute_batch(&format!(
                    "CREATE TRIGGER deny_{table}_{operation} BEFORE {operation} ON {table}
                 BEGIN SELECT RAISE(ABORT, 'unexpected reconciliation mutation'); END"
                ))
                .unwrap();
        }
    }
}

fn field_mut<'a>(value: &'a mut Value, name: &str) -> &'a mut Value {
    let Value::Map(fields) = value else {
        panic!("expected canonical record map")
    };
    &mut fields
        .iter_mut()
        .find(|(key, _)| key.as_text() == Some(name))
        .unwrap()
        .1
}

fn encode(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).unwrap();
    bytes
}

fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn replace_registration_receipt(path: &Path) {
    let connection = open_with_memory_limits(path).unwrap();
    let workspace: Vec<u8> = connection
        .query_row("SELECT record FROM jj_native_workspaces", [], |row| {
            row.get(0)
        })
        .unwrap();
    let source: Vec<u8> = connection
        .query_row("SELECT record FROM jj_native_registrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut workspace: Value = ciborium::from_reader(workspace.as_slice()).unwrap();
    let mut source: Value = ciborium::from_reader(source.as_slice()).unwrap();
    let attachment = "c".repeat(64);
    assert_ne!(
        field_mut(&mut workspace, "attachment_id").as_text(),
        Some(attachment.as_str())
    );
    *field_mut(&mut workspace, "attachment_id") = Value::Text(attachment.clone());
    let workspace = encode(&workspace);
    *field_mut(&mut source, "initial_attachment_id") = Value::Text(attachment);
    *field_mut(&mut source, "initial_workspace_record_id") = Value::Text(checksum(&workspace));
    let source = encode(&source);
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE jj_native_workspaces SET record=?1, checksum=?2",
                params![workspace, checksum(&workspace)]
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .execute(
                "UPDATE jj_native_registrations SET record=?1, checksum=?2",
                params![source, checksum(&source)]
            )
            .unwrap(),
        1
    );
    connection.execute_batch("COMMIT").unwrap();
}

pub(super) fn replace_admitted_heads(path: &Path) {
    let connection = open_with_memory_limits(path).unwrap();
    let (old_id, packet): (String, Vec<u8>) = connection
        .query_row(
            "SELECT admission_id,record FROM jj_native_admissions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let state: Vec<u8> = connection
        .query_row("SELECT state FROM jj_native_admission_states", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut packet: Value = ciborium::from_reader(packet.as_slice()).unwrap();
    let mut state: Value = ciborium::from_reader(state.as_slice()).unwrap();
    let mut heads = vec![
        fixtures::merge().operation_id,
        fixtures::rich_parent().operation_id,
    ];
    heads.sort();
    let heads: Vec<_> = heads.into_iter().map(Value::Text).collect();
    *field_mut(&mut packet, "captured_head_ids") = Value::Array(heads.clone());
    *field_mut(&mut state, "admitted_head_ids") = Value::Array(heads);
    let packet = encode(&packet);
    let id = checksum(&packet);
    assert_ne!(id, old_id);
    *field_mut(&mut state, "admission_id") = Value::Text(id.clone());
    let state = encode(&state);
    connection
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(connection.execute(
        "UPDATE jj_native_admissions SET admission_id=?1,record=?2,checksum=?1 WHERE admission_id=?3",
        params![id, packet, old_id]
    ).unwrap(), 1);
    assert_eq!(
        connection
            .execute(
                "UPDATE jj_native_admission_states SET admission_id=?1,state=?2,checksum=?3",
                params![id, state, checksum(&state)]
            )
            .unwrap(),
        1
    );
    connection.execute_batch("COMMIT").unwrap();
}
