use super::*;
use rusqlite::params;

#[test]
fn native_admission_model_fresh_cursor_is_derived_without_rows_and_preserves_baseline() {
    let fixture = Fixture::new();
    let before = fixture.old_rows();
    let journal = fixture.journal();
    let mut budget = unlimited();
    let saved = read(&journal, None, &mut budget).unwrap();
    assert_eq!(saved.cursor.generation, 0);
    assert_eq!(saved.cursor.admitted_head_ids, [vectors::FIRST]);
    assert_eq!(saved.registration.native.state.generation, 1);
    assert_eq!(saved.registration.registration.checksum, vectors::RECEIPT);
    assert!(saved.latest.is_none() && saved.requested().is_none());
    assert_eq!(budget.consumed(), fixture.registration_bytes());
    assert!(fixture.packet_rows().is_empty() && fixture.state_rows().is_empty());
    assert_eq!(fixture.old_rows(), before);
}

#[test]
fn native_admission_model_alias_selection_materializes_latest_once_and_moves_without_clone() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let journal = fixture.journal();
    let mut budget = unlimited();
    let saved = read(&journal, Some(vector("left").admission_id), &mut budget).unwrap();
    assert!(std::ptr::eq(
        saved.latest.as_ref().unwrap(),
        saved.requested().unwrap()
    ));
    let pointer = saved.requested().unwrap().record.operations[0]
        .operation_bytes
        .as_ptr();
    let expected =
        fixture.registration_bytes() + vector("left_state").raw.len() + vector("left").raw.len();
    assert_eq!(budget.consumed(), expected);
    let (registration, cursor, requested) = saved.into_requested();
    assert_eq!(registration.registration.checksum, vectors::RECEIPT);
    assert_eq!(cursor.generation, 1);
    let requested = requested.unwrap();
    assert_eq!(
        requested.record.operations[0].operation_bytes.as_ptr(),
        pointer
    );
    assert_eq!(requested.record.operations, Input::from("left").operations);
}

#[test]
fn native_admission_model_distinct_historical_selection_keeps_current_cursor_and_moves_request() {
    let fixture = Fixture::new();
    fixture.seed("merge");
    let journal = fixture.journal();
    let mut budget = unlimited();
    let saved = journal
        .read_native_admission_snapshot(
            vectors::SOURCE,
            None,
            Some(vector("left").admission_id),
            &mut budget,
        )
        .unwrap();
    assert_cursor(&saved, 2, "merge");
    assert_eq!(
        saved.latest.as_ref().unwrap().admission_id,
        vector("merge").admission_id
    );
    assert_eq!(saved.requested().unwrap().generation, 1);
    assert!(!std::ptr::eq(
        saved.latest.as_ref().unwrap(),
        saved.requested().unwrap()
    ));
    assert_eq!(
        budget.consumed(),
        fixture.registration_bytes()
            + vector("merge_state").raw.len()
            + vector("merge").raw.len()
            + vector("left").raw.len()
    );
    let pointer = saved.requested().unwrap().record.operations[0]
        .operation_bytes
        .as_ptr();
    let (_, cursor, requested) = saved.into_requested();
    assert_eq!(cursor.generation, 2);
    assert_eq!(
        requested.unwrap().record.operations[0]
            .operation_bytes
            .as_ptr(),
        pointer
    );
}

#[test]
fn native_admission_model_unknown_request_and_original_workspace_do_not_duplicate_reads() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let journal = fixture.journal();
    let expected =
        fixture.registration_bytes() + vector("left_state").raw.len() + vector("left").raw.len();
    for workspace in [None, Some("default")] {
        let mut budget = ReadBudget::new(expected);
        let saved = journal
            .read_native_admission_snapshot(
                vectors::SOURCE,
                workspace,
                Some(&"ab".repeat(32)),
                &mut budget,
            )
            .unwrap();
        assert_cursor(&saved, 1, "left");
        assert!(saved.requested().is_none());
        assert_eq!(budget.consumed(), expected);
    }
}

#[test]
fn native_admission_model_unselected_other_source_corruption_cannot_affect_known_source() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let conn = fixture.conn();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute(
        "INSERT INTO jj_native_admissions VALUES (?1,?2,?3,?4,?5)",
        params!["fe".repeat(32), "ff".repeat(32), 999, b"bad", "bad"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO jj_native_admission_states VALUES (?1,?2,?3,?4)",
        params!["fe".repeat(32), "ff".repeat(32), b"bad", "bad"],
    )
    .unwrap();
    let journal = fixture.journal();
    assert_cursor(&read(&journal, None, &mut unlimited()).unwrap(), 1, "left");
    assert!(
        journal
            .read_native_admission_snapshot(&"fe".repeat(32), None, None, &mut unlimited())
            .is_err()
    );
    assert!(
        journal
            .read_native_admission_snapshot(&"fb".repeat(32), None, None, &mut unlimited())
            .is_err()
    );
}

#[test]
fn native_admission_model_one_sided_gaps_and_future_packets_are_corruption() {
    for sql in [
        "DELETE FROM jj_native_admission_states",
        "DELETE FROM jj_native_admissions",
    ] {
        let fixture = Fixture::new();
        fixture.seed("left");
        let conn = fixture.conn();
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        conn.execute(sql, []).unwrap();
        assert!(
            read(&fixture.journal(), None, &mut unlimited()).is_err(),
            "{sql}"
        );
    }
    let fixture = Fixture::new();
    fixture.seed("left");
    fixture.insert("merge");
    assert!(
        read(
            &fixture.journal(),
            Some(vector("left").admission_id),
            &mut unlimited()
        )
        .is_err()
    );
}

#[test]
fn native_admission_model_current_generation_cardinality_is_checked_on_damaged_live_schema() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let journal = fixture.journal();
    let conn = fixture.conn();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute_batch("ALTER TABLE jj_native_admissions RENAME TO old_admissions;
      CREATE TABLE jj_native_admissions(source_id TEXT NOT NULL,admission_id TEXT NOT NULL,generation INTEGER NOT NULL,record BLOB NOT NULL,checksum TEXT NOT NULL,PRIMARY KEY(source_id,admission_id));
      INSERT INTO jj_native_admissions SELECT * FROM old_admissions;").unwrap();
    let extra = vector("baseline_only");
    conn.execute(
        "INSERT INTO jj_native_admissions VALUES (?1,?2,1,?3,?4)",
        params![
            vectors::SOURCE,
            extra.admission_id,
            extra.raw,
            extra.checksum
        ],
    )
    .unwrap();
    assert!(read(&journal, None, &mut unlimited()).is_err());
}

#[test]
fn native_admission_model_historical_generation_cardinality_is_checked_on_damaged_live_schema() {
    let fixture = Fixture::new();
    fixture.seed("merge");
    let journal = fixture.journal();
    let conn = fixture.conn();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute_batch("ALTER TABLE jj_native_admissions RENAME TO old_admissions;
      CREATE TABLE jj_native_admissions(source_id TEXT NOT NULL,admission_id TEXT NOT NULL,generation INTEGER NOT NULL,record BLOB NOT NULL,checksum TEXT NOT NULL,PRIMARY KEY(source_id,admission_id));
      INSERT INTO jj_native_admissions SELECT * FROM old_admissions;").unwrap();
    let extra = vector("baseline_only");
    conn.execute(
        "INSERT INTO jj_native_admissions VALUES (?1,?2,1,?3,?4)",
        params![
            vectors::SOURCE,
            extra.admission_id,
            extra.raw,
            extra.checksum
        ],
    )
    .unwrap();
    // Older unselected rows remain outside this bounded snapshot claim.
    assert_cursor(&read(&journal, None, &mut unlimited()).unwrap(), 2, "merge");
    assert!(
        read(
            &journal,
            Some(vector("left").admission_id),
            &mut unlimited()
        )
        .is_err()
    );
}

#[test]
fn native_admission_model_sql_generation_and_checksum_must_match_packet_identity() {
    for sql in [
        "UPDATE jj_native_admissions SET checksum='bad'",
        "UPDATE jj_native_admissions SET generation=0",
        "UPDATE jj_native_admissions SET generation=2",
        "UPDATE jj_native_admissions SET generation=1.5",
        "UPDATE jj_native_admissions SET generation='wrong-type'",
        "UPDATE jj_native_admissions SET generation=zeroblob(1048576)",
    ] {
        let fixture = Fixture::new();
        fixture.seed("left");
        fixture.conn().execute(sql, []).unwrap();
        assert!(
            read(&fixture.journal(), None, &mut unlimited()).is_err(),
            "{sql}"
        );
    }
}

#[test]
fn native_admission_model_checks_current_integrity_before_historical_absence_or_hit() {
    let fixture = Fixture::new();
    fixture.seed("merge");
    fixture
        .conn()
        .execute("UPDATE jj_native_admission_states SET checksum='bad'", [])
        .unwrap();
    for request in [vector("left").admission_id.to_owned(), "ab".repeat(32)] {
        assert!(read(&fixture.journal(), Some(&request), &mut unlimited()).is_err());
    }
}

#[test]
fn native_admission_model_both_family_erasure_is_explicitly_indistinguishable_from_first_use() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let conn = fixture.conn();
    conn.execute("DELETE FROM jj_native_admission_states", [])
        .unwrap();
    conn.execute("DELETE FROM jj_native_admissions", [])
        .unwrap();
    let saved = read(&fixture.journal(), None, &mut unlimited()).unwrap();
    assert_eq!(saved.cursor.generation, 0);
    assert_eq!(saved.cursor.admitted_head_ids, [vectors::FIRST]);
    assert_eq!(saved.registration.native.state.generation, 1);
}

#[test]
fn native_admission_model_canonical_state_heads_and_packet_scope_must_join_registration() {
    use ciborium::value::Value;
    for key in [
        "initialization_receipt_id",
        "baseline_id",
        "admitted_head_ids",
    ] {
        let fixture = Fixture::new();
        fixture.seed("left");
        let mut state = value(vector("left_state").raw);
        *field_mut(&mut state, key) = if key == "admitted_head_ids" {
            Value::Array(vec![Value::Text(vectors::FIRST.to_owned())])
        } else {
            Value::Text("ab".repeat(32))
        };
        let bytes = encode(&state);
        fixture
            .conn()
            .execute(
                "UPDATE jj_native_admission_states SET state=?1,checksum=?2",
                params![bytes, checksum(&bytes)],
            )
            .unwrap();
        assert!(
            read(&fixture.journal(), None, &mut unlimited()).is_err(),
            "{key}"
        );
    }
    for key in [
        "initialization_receipt_id",
        "baseline_id",
        "expected_admitted_head_ids",
    ] {
        let fixture = Fixture::new();
        let mut packet = value(vector("left").raw);
        let replacement = if key == "expected_admitted_head_ids" {
            field(&packet, "captured_head_ids").clone()
        } else {
            Value::Text("ab".repeat(32))
        };
        *field_mut(&mut packet, key) = replacement.clone();
        let bytes = encode(&packet);
        let identity = checksum(&bytes);
        let mut state = value(vector("left_state").raw);
        if key != "expected_admitted_head_ids" {
            *field_mut(&mut state, key) = replacement;
        }
        *field_mut(&mut state, "admission_id") = Value::Text(identity.clone());
        let state = encode(&state);
        let conn = fixture.conn();
        conn.execute(
            "INSERT INTO jj_native_admissions VALUES (?1,?2,1,?3,?4)",
            params![vectors::SOURCE, identity, bytes, identity],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO jj_native_admission_states VALUES (?1,?2,?3,?4)",
            params![vectors::SOURCE, identity, state, checksum(&state)],
        )
        .unwrap();
        assert!(
            read(&fixture.journal(), None, &mut unlimited()).is_err(),
            "{key}"
        );
    }
}

#[test]
fn native_admission_model_duplicate_live_state_rows_are_not_collapsed_to_the_first() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let journal = fixture.journal();
    let conn = fixture.conn();
    conn.execute_batch("ALTER TABLE jj_native_admission_states RENAME TO old_states;
      CREATE TABLE jj_native_admission_states(source_id TEXT NOT NULL,admission_id TEXT NOT NULL,state BLOB NOT NULL,checksum TEXT NOT NULL);
      CREATE INDEX damaged_state_source ON jj_native_admission_states(source_id);
      INSERT INTO jj_native_admission_states SELECT * FROM old_states;
      INSERT INTO jj_native_admission_states SELECT * FROM old_states;").unwrap();
    assert!(read(&journal, None, &mut unlimited()).is_err());
}
