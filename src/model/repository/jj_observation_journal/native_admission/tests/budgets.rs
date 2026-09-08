use super::*;
use rusqlite::params;

#[test]
fn native_admission_model_read_budget_is_reused_across_calls_and_rejects_before_materialization() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let journal = fixture.journal();
    let bytes =
        fixture.registration_bytes() + vector("left_state").raw.len() + vector("left").raw.len();
    let mut zero = ReadBudget::new(0);
    assert!(read(&journal, None, &mut zero).is_err());
    assert_eq!(zero.consumed(), 0);
    let mut exact = ReadBudget::new(bytes);
    read(&journal, None, &mut exact).unwrap();
    assert_eq!(exact.consumed(), bytes);
    assert!(read(&journal, None, &mut exact).is_err());
    assert_eq!(exact.consumed(), bytes);
    let mut short = ReadBudget::new(bytes - 1);
    assert!(read(&journal, None, &mut short).is_err());
    assert_eq!(short.consumed(), bytes - vector("left").raw.len());
}

#[test]
fn native_admission_model_selected_corrupt_packet_remains_charged_on_error() {
    let fixture = Fixture::new();
    fixture.seed("left");
    fixture
        .conn()
        .execute(
            "UPDATE jj_native_admissions SET record=CAST(X'00'||substr(record,2) AS BLOB)",
            [],
        )
        .unwrap();
    let expected =
        fixture.registration_bytes() + vector("left_state").raw.len() + vector("left").raw.len();
    let mut budget = ReadBudget::new(expected);
    assert!(read(&fixture.journal(), None, &mut budget).is_err());
    assert_eq!(budget.consumed(), expected);
    assert!(read(&fixture.journal(), None, &mut budget).is_err());
    assert_eq!(budget.consumed(), expected);
}

#[test]
fn native_admission_model_oversized_state_or_packet_does_not_select_its_payload() {
    for state in [true, false] {
        let fixture = Fixture::new();
        fixture.seed("left");
        let (sql, limit) = if state {
            (
                "UPDATE jj_native_admission_states SET state=zeroblob(?1)",
                128 * 1024,
            )
        } else {
            (
                "UPDATE jj_native_admissions SET record=zeroblob(?1)",
                8 * 1024 * 1024,
            )
        };
        fixture.conn().execute(sql, [limit + 1]).unwrap();
        let mut budget = unlimited();
        assert!(read(&fixture.journal(), None, &mut budget).is_err());
        let expected = fixture.registration_bytes()
            + if state {
                0
            } else {
                vector("left_state").raw.len()
            };
        assert_eq!(budget.consumed(), expected);
    }
}

#[test]
fn native_admission_model_exact_state_outer_ceiling_is_charged_before_invalid_cbor() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let bytes = vec![0; 128 * 1024];
    fixture
        .conn()
        .execute(
            "UPDATE jj_native_admission_states SET state=?1,checksum=?2",
            params![bytes, checksum(&bytes)],
        )
        .unwrap();
    let expected = fixture.registration_bytes() + bytes.len();
    let mut budget = ReadBudget::new(expected);
    assert!(read(&fixture.journal(), None, &mut budget).is_err());
    assert_eq!(budget.consumed(), expected);
}

#[test]
fn native_admission_model_wrong_payload_types_and_large_generation_metadata_are_refused() {
    for sql in [
        "UPDATE jj_native_admission_states SET state=CAST(state AS TEXT)",
        "UPDATE jj_native_admissions SET record=CAST(record AS TEXT)",
        "UPDATE jj_native_admissions SET generation=zeroblob(1048576)",
        "UPDATE jj_native_admissions SET generation=CAST(zeroblob(1048576) AS TEXT)",
    ] {
        let fixture = Fixture::new();
        fixture.seed("left");
        fixture.conn().execute(sql, []).unwrap();
        let ceiling = fixture.registration_bytes()
            + vector("left_state").raw.len()
            + vector("left").raw.len();
        let mut budget = ReadBudget::new(ceiling);
        assert!(
            read(&fixture.journal(), None, &mut budget).is_err(),
            "{sql}"
        );
        if sql.contains("SET state=") {
            assert_eq!(budget.consumed(), fixture.registration_bytes());
        } else if sql.contains("SET record=") {
            assert_eq!(
                budget.consumed(),
                fixture.registration_bytes() + vector("left_state").raw.len()
            );
        } else {
            assert!(budget.consumed() <= ceiling);
        }
    }
}

#[test]
fn native_admission_model_preflight_and_postwrite_share_one_budget_and_failed_readback_rolls_back()
{
    let fixture = Fixture::new();
    let input = Input::from("left");
    let prepared = input.prepare().unwrap();
    let id = prepared.admission_id().to_owned();
    let old = fixture.old_rows();
    let mut journal = fixture.journal();
    let mut budget = ReadBudget::new(fixture.registration_bytes());
    let transaction = journal
        .begin_native_admission(vectors::SOURCE, Some("default"), Some(&id), &mut budget)
        .unwrap();
    assert_eq!(budget.remaining(), 0);
    assert!(transaction.stage(prepared, &mut budget).is_err());
    assert_eq!(budget.consumed(), fixture.registration_bytes());
    assert!(fixture.packet_rows().is_empty() && fixture.state_rows().is_empty());
    assert_eq!(fixture.old_rows(), old);
}
