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
fn native_admission_segments_transaction_requires_the_current_receipt_link() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let input = Input::from("merge");
    let mut journal = fixture.journal();
    let packets = fixture.packet_rows();
    let states = fixture.state_rows();
    let wrong = "ab".repeat(32);
    let prepared = segment(&input, &wrong).unwrap();
    let id = prepared.admission_id().to_owned();
    let tx = journal
        .begin_native_admission(
            vectors::SOURCE,
            Some("default"),
            Some(&id),
            &mut unlimited(),
        )
        .unwrap();
    assert!(tx.stage(prepared, &mut unlimited()).is_err());
    assert_eq!(fixture.packet_rows(), packets);
    assert_eq!(fixture.state_rows(), states);

    // The same generation and heads are admissible with the exact current link.
    let prepared = segment(&input, vector("left").admission_id).unwrap();
    let id = prepared.admission_id().to_owned();
    let tx = journal
        .begin_native_admission(
            vectors::SOURCE,
            Some("default"),
            Some(&id),
            &mut unlimited(),
        )
        .unwrap();
    let (commit, outcome, _) = tx.stage(prepared, &mut unlimited()).unwrap().into_parts();
    assert_eq!(outcome, NativeAdmissionOutcome::Admitted);
    commit.commit().unwrap();
    assert_cursor(&read(&journal, None, &mut unlimited()).unwrap(), 2, "merge");

    // Exact v1 historical retries still preserve the newer linked cursor.
    let historical = Input::from("left");
    let prepared = historical.prepare().unwrap();
    let tx = journal
        .begin_native_admission(
            vectors::SOURCE,
            Some("default"),
            Some(prepared.admission_id()),
            &mut unlimited(),
        )
        .unwrap();
    let (commit, outcome, snapshot) = tx.stage(prepared, &mut unlimited()).unwrap().into_parts();
    assert_eq!(outcome, NativeAdmissionOutcome::AlreadyAdmitted);
    assert_cursor(&snapshot, 2, "merge");
    commit.commit().unwrap();
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
fn native_admission_segments_writer_rejects_schema_drift_before_reading_packets() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let mut journal = fixture.journal();
    // The already-open connection must not scan history after losing its
    // generation index, even when the selected rows remain perfectly valid.
    fixture
        .conn()
        .execute_batch(
            "PRAGMA foreign_keys=OFF;
         CREATE TABLE packet_copy AS SELECT * FROM jj_native_admissions;
         DROP TABLE jj_native_admissions;
         CREATE TABLE jj_native_admissions (
            source_id TEXT NOT NULL, admission_id TEXT NOT NULL,
            generation INTEGER NOT NULL, record BLOB NOT NULL, checksum TEXT NOT NULL,
            PRIMARY KEY (source_id, admission_id),
            FOREIGN KEY (source_id) REFERENCES jj_native_registrations(source_id));
         INSERT INTO jj_native_admissions SELECT * FROM packet_copy;
         DROP TABLE packet_copy;",
        )
        .unwrap();
    let mut budget = unlimited();
    let result =
        journal.begin_native_admission(vectors::SOURCE, Some("default"), None, &mut budget);
    assert!(result.is_err());
    assert_eq!(budget.consumed(), 0);
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
