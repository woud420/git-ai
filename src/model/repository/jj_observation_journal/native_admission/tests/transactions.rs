use super::*;

#[test]
fn native_admission_model_begin_is_readonly_and_drop_preserves_all_rows() {
    let fixture = Fixture::new();
    fixture.seed("left");
    let old = fixture.old_rows();
    let packets = fixture.packet_rows();
    let states = fixture.state_rows();
    let mut journal = fixture.journal();
    let transaction = journal
        .begin_native_admission(
            vectors::SOURCE,
            None,
            Some(vector("left").admission_id),
            &mut unlimited(),
        )
        .unwrap();
    assert_cursor(transaction.snapshot(), 1, "left");
    assert_eq!(fixture.packet_rows(), packets);
    assert_eq!(fixture.state_rows(), states);
    drop(transaction);
    assert_eq!(fixture.packet_rows(), packets);
    assert_eq!(fixture.state_rows(), states);
    assert_eq!(fixture.old_rows(), old);
}

#[test]
fn native_admission_model_staged_commit_token_owns_visibility_and_rollback() {
    let fixture = Fixture::new();
    let old = fixture.old_rows();
    let mut journal = fixture.journal();
    for commit_it in [false, true] {
        let input = Input::from("left");
        let prepared = input.prepare().unwrap();
        let id = prepared.admission_id().to_owned();
        let transaction = journal
            .begin_native_admission(
                vectors::SOURCE,
                Some("default"),
                Some(&id),
                &mut unlimited(),
            )
            .unwrap();
        let staged = transaction.stage(prepared, &mut unlimited()).unwrap();
        let (token, outcome, saved) = staged.into_parts();
        assert_eq!(outcome, NativeAdmissionOutcome::Admitted);
        assert_cursor(&saved, 1, "left");
        let (_, cursor, selected) = saved.into_requested();
        assert_eq!(cursor.generation, 1);
        assert_eq!(selected.unwrap().record.operations, input.operations);
        assert!(fixture.packet_rows().is_empty() && fixture.state_rows().is_empty());
        if commit_it {
            token.commit().unwrap();
        } else {
            drop(token);
        }
    }
    assert_eq!(fixture.packet_rows().len(), 1);
    assert_eq!(fixture.state_rows().len(), 1);
    assert_eq!(fixture.old_rows(), old);
    let conn = fixture.conn();
    let raw: Vec<u8> = conn
        .query_row("SELECT record FROM jj_native_admissions", [], |row| {
            row.get(0)
        })
        .unwrap();
    let state: Vec<u8> = conn
        .query_row("SELECT state FROM jj_native_admission_states", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(raw, vector("left").raw);
    assert_eq!(state, vector("left_state").raw);
}

#[test]
fn native_admission_model_exact_historical_receipt_precedes_cas_and_never_rewinds() {
    let fixture = Fixture::new();
    committed(&fixture, "left");
    committed(&fixture, "merge");
    let packets = fixture.packet_rows();
    let states = fixture.state_rows();
    let old = fixture.old_rows();
    let input = Input::from("left");
    let prepared = input.prepare().unwrap();
    let id = prepared.admission_id().to_owned();
    let mut journal = fixture.journal();
    let mut budget = unlimited();
    let transaction = journal
        .begin_native_admission(vectors::SOURCE, None, Some(&id), &mut budget)
        .unwrap();
    assert_cursor(transaction.snapshot(), 2, "merge");
    let before_stage = budget.consumed();
    let staged = transaction.stage(prepared, &mut budget).unwrap();
    let (token, outcome, saved) = staged.into_parts();
    assert_eq!(outcome, NativeAdmissionOutcome::AlreadyAdmitted);
    assert_cursor(&saved, 2, "merge");
    assert_eq!(saved.requested().unwrap().generation, 1);
    assert_eq!(budget.consumed(), before_stage);
    token.commit().unwrap();
    assert_eq!(fixture.packet_rows(), packets);
    assert_eq!(fixture.state_rows(), states);
    assert_eq!(fixture.old_rows(), old);
}

#[test]
fn native_admission_model_generations_distinguish_repeated_heads_and_stale_new_requests() {
    let fixture = Fixture::new();
    committed(&fixture, "left");
    committed(&fixture, "merge");
    committed(&fixture, "return_left");
    let journal = fixture.journal();
    let current = read(&journal, None, &mut unlimited()).unwrap();
    assert_cursor(&current, 3, "left");
    assert_ne!(
        vector("return_left").admission_id,
        vector("left").admission_id
    );
    drop(journal);
    let old = fixture.packet_rows();
    let mut input = Input::from("baseline_only");
    input.heads = Input::from("left").heads;
    input.operations = Input::from("left").operations;
    input.generation = 1;
    input.expected = Input::from("left").heads;
    let prepared = input.prepare().unwrap();
    let id = prepared.admission_id().to_owned();
    let mut journal = fixture.journal();
    let transaction = journal
        .begin_native_admission(vectors::SOURCE, None, Some(&id), &mut unlimited())
        .unwrap();
    assert!(transaction.stage(prepared, &mut unlimited()).is_err());
    assert_eq!(fixture.packet_rows(), old);
}

#[test]
fn native_admission_model_stage_cannot_replace_transaction_scope_or_selected_request() {
    for change in [0, 1, 2, 3] {
        let fixture = Fixture::new();
        let mut input = Input::from("left");
        match change {
            0 => input.source = "ab".repeat(32),
            1 => input.receipt = "ab".repeat(32),
            2 => input.baseline = "ab".repeat(32),
            _ => {}
        }
        let prepared = input.prepare().unwrap();
        let requested = if change == 3 {
            "cd".repeat(32)
        } else {
            prepared.admission_id().to_owned()
        };
        let mut journal = fixture.journal();
        let transaction = journal
            .begin_native_admission(
                vectors::SOURCE,
                Some("default"),
                Some(&requested),
                &mut unlimited(),
            )
            .unwrap();
        assert!(transaction.stage(prepared, &mut unlimited()).is_err());
        assert!(fixture.packet_rows().is_empty() && fixture.state_rows().is_empty());
    }
}

#[test]
fn native_admission_model_nonzero_expected_cursor_cannot_recreate_erased_history() {
    let fixture = Fixture::new();
    committed(&fixture, "left");
    let conn = fixture.conn();
    conn.execute("DELETE FROM jj_native_admission_states", [])
        .unwrap();
    conn.execute("DELETE FROM jj_native_admissions", [])
        .unwrap();
    let input = Input::from("merge");
    let prepared = input.prepare().unwrap();
    let id = prepared.admission_id().to_owned();
    let mut journal = fixture.journal();
    let transaction = journal
        .begin_native_admission(vectors::SOURCE, None, Some(&id), &mut unlimited())
        .unwrap();
    assert_eq!(transaction.snapshot().cursor.generation, 0);
    assert!(transaction.stage(prepared, &mut unlimited()).is_err());
    assert!(fixture.packet_rows().is_empty() && fixture.state_rows().is_empty());
}

#[test]
fn native_admission_model_two_prepared_writers_are_serialized_by_generation_cas() {
    let fixture = Fixture::new();
    let first_input = Input::from("left");
    let second_input = Input::from("baseline_only");
    let first = first_input.prepare().unwrap();
    let second = second_input.prepare().unwrap();
    let first_id = first.admission_id().to_owned();
    let second_id = second.admission_id().to_owned();
    let mut left = fixture.journal();
    let mut right = fixture.journal();
    let transaction = left
        .begin_native_admission(vectors::SOURCE, None, Some(&first_id), &mut unlimited())
        .unwrap();
    let (commit, _, _) = transaction
        .stage(first, &mut unlimited())
        .unwrap()
        .into_parts();
    commit.commit().unwrap();
    let transaction = right
        .begin_native_admission(vectors::SOURCE, None, Some(&second_id), &mut unlimited())
        .unwrap();
    assert!(transaction.stage(second, &mut unlimited()).is_err());
    assert_eq!(fixture.packet_rows().len(), 1);
    assert_cursor(&read(&right, None, &mut unlimited()).unwrap(), 1, "left");
}

#[test]
fn native_admission_model_begin_reserves_immediate_transaction_before_native_prevalidation() {
    let fixture = Fixture::new();
    let mut journal = fixture.journal();
    let transaction = journal
        .begin_native_admission(vectors::SOURCE, None, None, &mut unlimited())
        .unwrap();
    let competing = fixture.conn();
    competing.busy_timeout(std::time::Duration::ZERO).unwrap();
    let error = competing.execute_batch("BEGIN IMMEDIATE").unwrap_err();
    assert!(matches!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    ));
    drop(transaction);
    competing
        .execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
        .unwrap();
    assert!(fixture.packet_rows().is_empty() && fixture.state_rows().is_empty());
}
