use super::*;

#[test]
fn jj_registration_records_each_read_requires_its_exact_selected_byte_allowance() {
    for record in [vector("linux_source"), vector("linux_workspace")] {
        let (fixture, conn) = record_fixture();
        insert_record(&conn, record);
        let journal = fixture.open();
        for allowance in [0, record.raw.len() - 1, record.raw.len()] {
            let mut budget = ReadBudget::new(allowance);
            let result = checked_read(&journal, &conn, record, &mut budget);
            if allowance == record.raw.len() {
                assert_eq!(result.unwrap(), Some(record.raw.to_vec()));
                assert_eq!(budget.consumed(), record.raw.len());
            } else {
                assert!(result.is_err());
                assert_eq!(budget.consumed(), 0);
                assert_eq!(budget.remaining(), allowance);
            }
        }
    }
}

#[test]
fn jj_registration_records_repeated_and_mixed_calls_share_one_budget() {
    let source = vector("linux_source");
    let workspace = vector("linux_workspace");
    let (fixture, conn) = record_fixture();
    insert_record(&conn, source);
    insert_record(&conn, workspace);
    let journal = fixture.open();
    let total = source.raw.len() * 2 + workspace.raw.len();
    let mut budget = ReadBudget::new(total);
    let mut consumed = 0;
    for record in [source, workspace, source] {
        assert_eq!(
            checked_read(&journal, &conn, record, &mut budget).unwrap(),
            Some(record.raw.to_vec())
        );
        consumed += record.raw.len();
        assert_eq!(budget.consumed(), consumed);
    }
    assert_eq!(budget.remaining(), 0);
    assert!(checked_read(&journal, &conn, workspace, &mut budget).is_err());
    assert_eq!(budget.consumed(), total);
}

#[test]
fn jj_registration_records_short_later_read_preserves_an_existing_charge() {
    let source = vector("linux_source");
    let workspace = vector("linux_workspace");
    let (fixture, conn) = record_fixture();
    insert_record(&conn, source);
    insert_record(&conn, workspace);
    let journal = fixture.open();
    let mut budget = ReadBudget::new(source.raw.len() + workspace.raw.len() - 1);
    assert!(
        checked_read(&journal, &conn, source, &mut budget)
            .unwrap()
            .is_some()
    );
    assert!(checked_read(&journal, &conn, workspace, &mut budget).is_err());
    assert_eq!(budget.consumed(), source.raw.len());
    assert_eq!(budget.remaining(), workspace.raw.len() - 1);
}

#[test]
fn jj_registration_records_failed_validation_keeps_prior_and_repeated_charges() {
    let source = vector("linux_source");
    let workspace = vector("linux_workspace");
    let (fixture, conn) = record_fixture();
    insert_record(&conn, source);
    insert_record(&conn, workspace);
    conn.execute("UPDATE jj_native_workspaces SET checksum = 'invalid'", [])
        .unwrap();
    let journal = fixture.open();
    let total = source.raw.len() + workspace.raw.len() * 2;
    let mut budget = ReadBudget::new(total);
    assert!(
        checked_read(&journal, &conn, source, &mut budget)
            .unwrap()
            .is_some()
    );
    for attempt in 1..=2 {
        assert!(checked_read(&journal, &conn, workspace, &mut budget).is_err());
        assert_eq!(
            budget.consumed(),
            source.raw.len() + workspace.raw.len() * attempt
        );
    }
    assert_eq!(budget.remaining(), 0);
    assert!(checked_read(&journal, &conn, source, &mut budget).is_err());
    assert_eq!(budget.consumed(), total);
}

#[test]
fn jj_registration_records_wrong_blob_types_and_outer_limit_fail_before_charge() {
    let mut exact = Vec::new();
    ciborium::into_writer(
        &Cbor::Map(vec![(
            Cbor::Text("padding".to_owned()),
            Cbor::Bytes(vec![0; RECORD_LIMIT - 14]),
        )]),
        &mut exact,
    )
    .unwrap();
    assert_eq!(exact.len(), RECORD_LIMIT);
    let mut oversized = exact.clone();
    oversized.push(0);
    for record in [vector("linux_source"), vector("linux_workspace")] {
        let (fixture, conn) = record_fixture();
        insert_record(&conn, record);
        let journal = fixture.open();
        unconstrain_after_open(&conn, record);
        for bad in [
            Value::Null,
            Value::Integer(7),
            Value::Real(1.5),
            Value::Text("not a blob".to_owned()),
            Value::Blob(oversized.clone()),
        ] {
            conn.execute(&format!("UPDATE {} SET record = ?1", table(record)), [bad])
                .unwrap();
            let mut budget = ReadBudget::new(RECORD_LIMIT * 2);
            assert!(checked_read(&journal, &conn, record, &mut budget).is_err());
            assert_eq!(budget.consumed(), 0);
        }
        replace_raw(&conn, record, &exact);
        assert_charged_error(&journal, &conn, record, RECORD_LIMIT);
    }
}
