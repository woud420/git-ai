use super::*;
use ciborium::Value;

fn segment<'a>(
    input: &'a Input,
    previous: &'a str,
) -> Result<PreparedNativeAdmission<'a>, JournalError> {
    let refs: Vec<_> = input.operations.iter().collect();
    PreparedNativeAdmission::new_segment(
        NativeAdmissionScope {
            source_id: &input.source,
            reader_profile: &input.profile,
            initialization_receipt_id: &input.receipt,
            baseline_id: &input.baseline,
            baseline_generation: input.baseline_generation,
        },
        ExpectedAdmissionCursor {
            generation: input.generation,
            admitted_head_ids: &input.expected,
        },
        &input.heads,
        &refs,
        previous,
    )
}

#[test]
fn native_admission_segments_keep_v1_bytes_and_distinct_canonical_domains() {
    let mut input = Input::from("left");
    input.generation = 1;
    let v1 = input.prepare().unwrap();
    assert_eq!(v1.record_bytes, input.bytes());
    let previous = "ab".repeat(32);
    let v2 = segment(&input, &previous).unwrap();
    assert_ne!(v1.admission_id(), v2.admission_id());
    let record = value(&v2.record_bytes);
    assert_eq!(number(field(&record, "record_version")), 2);
    assert_eq!(
        text(field(&record, "domain")),
        "git-ai/jj/native-admission/segment/v2"
    );
    assert_eq!(text(field(&record, "previous_admission_id")), previous);
    let decoded =
        admission_codec::decode_packet(&v2.record_bytes, &input.source, v2.admission_id(), 2)
            .unwrap();
    assert!(v2.request.matches(&decoded.record));
    assert!(!v1.request.matches(&decoded.record));
}

#[test]
fn native_admission_segments_require_a_positive_generation_and_exact_receipt_id() {
    let mut input = Input::from("left");
    assert_eq!(input.generation, 0);
    assert!(segment(&input, &"ab".repeat(32)).is_err());
    input.generation = 1;
    for previous in [
        "".to_owned(),
        "ab".repeat(31),
        "AB".repeat(32),
        "g".repeat(64),
    ] {
        assert!(segment(&input, &previous).is_err());
    }
}

#[test]
fn native_admission_segments_reject_downgrade_unknown_version_and_zero_generation() {
    let mut input = Input::from("left");
    input.generation = 1;
    let previous = "ab".repeat(32);
    let prepared = segment(&input, &previous).unwrap();
    for change in ["downgrade", "unknown", "zero", "missing"] {
        let mut record = value(&prepared.record_bytes);
        match change {
            "downgrade" => {
                *field_mut(&mut record, "record_version") = Value::Integer(1.into());
                *field_mut(&mut record, "domain") =
                    Value::Text("git-ai/jj/native-admission/packet/v1".into());
            }
            "unknown" => *field_mut(&mut record, "record_version") = Value::Integer(3.into()),
            "zero" => {
                *field_mut(&mut record, "expected_admission_generation") = Value::Integer(0.into())
            }
            "missing" => *field_mut(&mut record, "previous_admission_id") = Value::Null,
            _ => unreachable!(),
        }
        let raw = encode(&record);
        assert!(
            admission_codec::decode_packet(&raw, &input.source, &checksum(&raw), 2).is_err(),
            "{change}"
        );
    }
}

#[test]
fn native_admission_segments_recheck_epoch_after_the_writer_reservation() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let mut journal = fixture.journal();
    let epoch = super::super::replay::data_version(&journal.conn).unwrap();
    fixture
        .conn()
        .execute_batch("CREATE TABLE unrelated_epoch_probe(value INTEGER)")
        .unwrap();
    let tx = journal
        .begin_native_admission(vectors::SOURCE, Some("default"), None, &mut unlimited())
        .unwrap();
    let error = tx.require_replay_epoch(epoch).unwrap_err();
    assert!(error.to_string().contains("connection changed"));
    drop(tx);
    let epoch = super::super::replay::data_version(&journal.conn).unwrap();
    let tx = journal
        .begin_native_admission(vectors::SOURCE, Some("default"), None, &mut unlimited())
        .unwrap();
    tx.require_replay_epoch(epoch).unwrap();
}

#[test]
fn native_admission_segments_refuse_own_temp_triggers_without_an_epoch_change() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let mut journal = fixture.journal();
    let epoch = super::super::replay::data_version(&journal.conn).unwrap();
    journal.conn.execute_batch("CREATE TEMP TRIGGER cached_prefix_mutation BEFORE UPDATE ON main.jj_native_admissions BEGIN SELECT 1; END").unwrap();
    assert_eq!(
        epoch,
        super::super::replay::data_version(&journal.conn).unwrap()
    );
    let tx = journal
        .begin_native_admission(vectors::SOURCE, Some("default"), None, &mut unlimited())
        .unwrap();
    let error = tx.require_replay_epoch(epoch).unwrap_err();
    assert!(error.to_string().contains("refuses triggers"));
}
