use super::*;
use ciborium::Value;

pub(super) fn records() -> Vec<JjOperationEvidence> {
    let a = native::rich_parent();
    let b = native::chain(1).remove(0);
    assert_ne!(a.operation_id, b.operation_id);
    for record in [&a, &b] {
        assert_eq!(record.parent_ids, [MERGE_ID]);
        verify_evidence(JJ_OBSERVATION_READER_PROFILE, record).unwrap();
    }
    vec![a, b]
}

pub(super) fn heads(records: &[JjOperationEvidence], both: bool) -> Vec<&str> {
    let mut heads = vec![records[0].operation_id.as_str()];
    if both {
        heads.push(records[1].operation_id.as_str());
    }
    heads.sort();
    heads
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn ids<'a>(values: impl IntoIterator<Item = &'a str>) -> Value {
    Value::Array(values.into_iter().map(text).collect())
}

fn map(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Map(
        fields
            .into_iter()
            .map(|(key, value)| (text(key), value))
            .collect(),
    )
}

fn encode(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).unwrap();
    bytes
}

fn v1(
    saved: &RegisteredJjCurrentState,
    expected: &NativeAdmissionCursor,
    ordered: &[JjOperationEvidence],
) -> Vec<u8> {
    let operations = ordered.iter().map(|record| {
        map([
            ("operation_id", text(&record.operation_id)),
            (
                "parent_ids",
                ids(record.parent_ids.iter().map(String::as_str)),
            ),
            ("view_id", text(&record.view_id)),
            (
                "operation_bytes",
                Value::Bytes(record.operation_bytes.clone()),
            ),
            ("view_bytes", Value::Bytes(record.view_bytes.clone())),
        ])
    });
    encode(&map([
        ("record_version", Value::Integer(1.into())),
        ("domain", text("git-ai/jj/native-admission/packet/v1")),
        ("source_id", text(saved.source_id())),
        ("reader_profile", text(JJ_OBSERVATION_READER_PROFILE)),
        (
            "initialization_receipt_id",
            text(saved.initialization_receipt_id()),
        ),
        (
            "baseline_id",
            text(saved.baseline().receipt().baseline_id()),
        ),
        ("baseline_generation", Value::Integer(1.into())),
        (
            "expected_admission_generation",
            Value::Integer(expected.generation().into()),
        ),
        (
            "expected_admitted_head_ids",
            ids(expected.admitted_head_ids().iter().map(String::as_str)),
        ),
        (
            "captured_head_ids",
            ids(ordered.iter().map(|record| record.operation_id.as_str())),
        ),
        ("operations", Value::Array(operations.collect())),
    ]))
}

pub(super) fn assert_packet(
    case: &Case,
    admission: &DurableNativeAdmission,
    saved: &RegisteredJjCurrentState,
    expected: &NativeAdmissionCursor,
    records: &[JjOperationEvidence],
    both: bool,
) -> Vec<u8> {
    let selected_heads = heads(records, both);
    let mut ordered = records[..if both { 2 } else { 1 }].to_vec();
    ordered.sort_by(|a, b| a.operation_id.cmp(&b.operation_id));
    assert_admission(
        case,
        admission,
        saved,
        expected,
        &selected_heads,
        &ordered,
        (&[MERGE_ID], false),
    );
    assert_eq!(admission.head_closures().len(), selected_heads.len());
    for (closure, head) in admission.head_closures().iter().zip(&selected_heads) {
        assert_eq!(closure.head_id(), *head);
        assert_eq!(closure.reached_baseline_ids(), [MERGE_ID]);
        assert!(!closure.reaches_root());
    }
    let expected_raw = v1(saved, expected, &ordered);
    let (raw, checksum, generation): (Vec<u8>, String, u64) = case.sql().query_row(
        "SELECT record,checksum,generation FROM jj_native_admissions WHERE source_id COLLATE BINARY=?1 AND admission_id COLLATE BINARY=?2",
        [saved.source_id(), admission.receipt().admission_id()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert!(
        raw == expected_raw,
        "default writer changed canonical full-v1 packet bytes"
    );
    assert_eq!(hash(&expected_raw), admission.receipt().admission_id());
    assert_eq!(checksum, admission.receipt().admission_id());
    assert_eq!(generation, expected.generation() + 1);
    expected_raw
}

fn field_mut<'a>(value: &'a mut Value, name: &str) -> &'a mut Value {
    let Value::Map(fields) = value else {
        panic!("state is not a map")
    };
    &mut fields
        .iter_mut()
        .find(|(key, _)| key.as_text() == Some(name))
        .unwrap()
        .1
}

pub(super) fn exact_write_bytes(
    case: &Case,
    saved: &RegisteredJjCurrentState,
    expected: &NativeAdmissionCursor,
    records: &[JjOperationEvidence],
    both: bool,
) -> usize {
    assert!(expected.generation() > 0);
    let conn = case.sql();
    let registration: usize = [
        ("jj_native_registrations", "record"),
        ("jj_native_workspaces", "record"),
        ("jj_native_baselines", "record"),
        ("jj_native_sources", "state"),
    ]
    .into_iter()
    .map(|(table, column)| {
        let (count, bytes): (usize, usize) = conn
            .query_row(
                &format!("SELECT COUNT(*),sum(length({column})) FROM {table} WHERE source_id=?1"),
                [saved.source_id()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "budget fixture requires original selected workspace only"
        );
        bytes
    })
    .sum();
    let (old_state, old_packet): (Vec<u8>, usize) = conn.query_row(
        "SELECT s.state,length(a.record) FROM jj_native_admission_states s JOIN jj_native_admissions a ON a.source_id=s.source_id AND a.admission_id=s.admission_id WHERE s.source_id=?1",
        [saved.source_id()], |row| Ok((row.get(0)?,row.get(1)?)),
    ).unwrap();
    let mut ordered = records[..if both { 2 } else { 1 }].to_vec();
    ordered.sort_by(|a, b| a.operation_id.cmp(&b.operation_id));
    let new_packet = v1(saved, expected, &ordered);
    let mut new_state: Value = ciborium::from_reader(old_state.as_slice()).unwrap();
    *field_mut(&mut new_state, "admission_id") = text(&hash(&new_packet));
    *field_mut(&mut new_state, "generation") = Value::Integer((expected.generation() + 1).into());
    *field_mut(&mut new_state, "admitted_head_ids") = ids(heads(records, both));
    // Initial registration; preflight registration/state/latest; readback registration/new state/packet.
    3 * registration + old_state.len() + old_packet + encode(&new_state).len() + new_packet.len()
}

pub(super) fn write_attempt(
    case: &Case,
    journal: &mut JjObservationJournal,
    config: &Config,
    saved: &RegisteredJjCurrentState,
    expected: &NativeAdmissionCursor,
    reconcile: bool,
    reads: &mut ReadBudget,
) -> Result<NativeAdmissionOutcome, JjNativeAdmissionError> {
    if !reconcile {
        return admit_checked(
            case,
            journal,
            &case.context(),
            config,
            expected.expectation(),
            deadline(),
            reads,
        );
    }
    let before = total_state(case);
    let old = state(case);
    let home = observe_home(&case.test_home);
    let result = reconcile_registered_history(
        journal,
        &case.context(),
        config,
        NativeReconciliationExpectation {
            admission: expected.expectation(),
            workspace_name: saved.workspace_name(),
            attachment_id: saved.attachment_id(),
        },
        deadline(),
        reads,
    );
    assert_eq!(state(case), old);
    assert_eq!(observe_home(&case.test_home), home);
    if !matches!(
        &result,
        Ok(NativeReconciliationOutcome::Admission(
            NativeAdmissionOutcome::Admitted(_)
        ))
    ) {
        assert!(
            total_state(case) == before,
            "nonwriting attempt changed stored state"
        );
    }
    result.map(|value| match value {
        NativeReconciliationOutcome::Admission(value) => value,
        NativeReconciliationOutcome::Unchanged(_) => panic!("changed heads were coalesced"),
    })
}

pub(super) fn require_rows(case: &Case, count: usize) {
    assert_eq!(
        admission_rows(case)
            .iter()
            .map(|(_, rows)| rows.len())
            .collect::<Vec<_>>(),
        [count, 1]
    );
}
