use super::*;

#[test]
fn jj_registration_records_selected_scalar_failures_keep_the_blob_charge() {
    for (record, columns) in [
        (
            vector("linux_source"),
            &["baseline_id", "source_root_key", "checksum"][..],
        ),
        (
            vector("linux_workspace"),
            &["locator_key", "workspace_root_key", "checksum"][..],
        ),
    ] {
        let (fixture, conn) = record_fixture();
        insert_record(&conn, record);
        let journal = fixture.open();
        unconstrain_after_open(&conn, record);
        for column in columns {
            for bad in [
                Value::Null,
                Value::Blob(b"a".repeat(64)),
                Value::Integer(7),
                Value::Real(1.5),
                Value::Text("bad".to_owned()),
                Value::Text("f".repeat(64)),
                Value::Text("a".repeat(65)),
                Value::Text("é".repeat(33)),
            ] {
                reset_record(&conn, record);
                conn.execute(
                    &format!("UPDATE {} SET {column} = ?1", table(record)),
                    [bad],
                )
                .unwrap();
                assert_charged_error(&journal, &conn, record, record.raw.len());
            }
        }
    }
}

#[test]
fn jj_registration_records_valid_selected_keys_must_match_the_record() {
    for record in [vector("linux_source"), vector("linux_workspace")] {
        let (fixture, conn) = record_fixture();
        insert_record(&conn, record);
        let journal = fixture.open();
        let mut fields = vec![("source_id", source_id(444))];
        if record.kind == "source" {
            fields.push(("baseline_id", source_id(445)));
        }
        for (field, replacement) in fields {
            reset_record(&conn, record);
            let changed = change_text(record.raw, field, &replacement);
            replace_raw(&conn, record, &changed);
            assert_charged_error(&journal, &conn, record, changed.len());
        }
        if record.kind == "workspace" {
            reset_record(&conn, record);
            let changed = change_text(record.raw, "workspace_name", "different");
            replace_raw(&conn, record, &changed);
            assert_charged_error(&journal, &conn, record, changed.len());
        }
        reset_record(&conn, record);
        let mut budget = ReadBudget::new(record.raw.len());
        let alternate = source_id(446);
        conn.execute(
            &format!("UPDATE {} SET source_id = ?1", table(record)),
            [&alternate],
        )
        .unwrap();
        let before = complete_snapshot(&conn);
        let result = if record.kind == "source" {
            journal.read_native_registration_record(&alternate, &mut budget)
        } else {
            journal.read_native_workspace_record(&alternate, record.workspace_name, &mut budget)
        };
        assert!(result.is_err());
        assert_eq!(budget.consumed(), record.raw.len());
        assert_eq!(complete_snapshot(&conn), before);
    }
}

#[test]
fn jj_registration_records_invalid_requests_fail_before_missing_tables_are_queried() {
    let (fixture, conn) = record_fixture();
    let record = vector("linux_source");
    insert_record(&conn, record);
    let journal = fixture.open();
    let mut budget = ReadBudget::new(record.raw.len() * 2);
    assert!(
        checked_read(&journal, &conn, record, &mut budget)
            .unwrap()
            .is_some()
    );
    conn.execute_batch("DROP TABLE jj_native_workspaces; DROP TABLE jj_native_registrations;")
        .unwrap();
    let before = complete_snapshot(&conn);
    for source in [
        String::new(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        assert!(matches!(
            journal.read_native_registration_record(&source, &mut budget),
            Err(JournalError::Validation(_))
        ));
        assert!(matches!(
            journal.read_native_workspace_record(&source, "default", &mut budget),
            Err(JournalError::Validation(_))
        ));
    }
    for name in [
        String::new(),
        "a".repeat(16 * 1024 + 1),
        "é".repeat(8 * 1024 + 1),
    ] {
        assert!(matches!(
            journal.read_native_workspace_record(record.source_id, &name, &mut budget),
            Err(JournalError::Validation(_))
        ));
    }
    assert_eq!(budget.consumed(), record.raw.len());
    assert_eq!(complete_snapshot(&conn), before);
}

#[test]
fn jj_registration_records_wrong_type_indexed_keys_mean_exact_row_absence() {
    for record in [vector("linux_source"), vector("linux_workspace")] {
        let (fixture, conn) = record_fixture();
        insert_record(&conn, record);
        let journal = fixture.open();
        unconstrain_after_open(&conn, record);
        let (column, key) = if record.kind == "source" {
            ("source_id", record.source_id)
        } else {
            ("workspace_name", record.workspace_name)
        };
        for bad in [
            Value::Blob(key.as_bytes().to_vec()),
            Value::Null,
            Value::Text("not-the-key".to_owned()),
        ] {
            reset_record(&conn, record);
            conn.execute(
                &format!("UPDATE {} SET {column} = ?1", table(record)),
                [bad],
            )
            .unwrap();
            let mut budget = ReadBudget::new(0);
            assert!(
                checked_read(&journal, &conn, record, &mut budget)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(budget.consumed(), 0);
        }
    }
}

#[test]
fn jj_registration_records_duplicate_keys_after_open_reject_before_payload_selection() {
    for record in [vector("linux_source"), vector("linux_workspace")] {
        let (fixture, conn) = record_fixture();
        insert_record(&conn, record);
        let journal = fixture.open();
        unconstrain_after_open(&conn, record);
        let table = table(record);
        conn.execute(&format!("INSERT INTO {table} SELECT * FROM {table}"), [])
            .unwrap();
        conn.execute(
            &format!(
                "UPDATE {table} SET record = zeroblob(?1), checksum = 'invalid' WHERE rowid = 2"
            ),
            [RECORD_LIMIT + 1],
        )
        .unwrap();
        let count: usize = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
        let mut budget = ReadBudget::new(record.raw.len());
        assert!(checked_read(&journal, &conn, record, &mut budget).is_err());
        assert_eq!(budget.consumed(), 0);
        let before = complete_snapshot(&conn);
        let absent = source_id(987);
        let result = if record.kind == "source" {
            journal.read_native_registration_record(&absent, &mut budget)
        } else {
            journal.read_native_workspace_record(record.source_id, "absent", &mut budget)
        };
        assert!(result.unwrap().is_none());
        assert_eq!(budget.consumed(), 0);
        assert_eq!(complete_snapshot(&conn), before);
    }
}
