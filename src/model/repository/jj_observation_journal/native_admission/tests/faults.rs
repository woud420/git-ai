use super::*;

fn reject_stage(fixture: &Fixture, label: &str) {
    let old = fixture.old_rows();
    let packets = fixture.packet_rows();
    let states = fixture.state_rows();
    let input = Input::from(label);
    let prepared = input.prepare().unwrap();
    let id = prepared.admission_id().to_owned();
    let mut journal = fixture.journal();
    let transaction = journal
        .begin_native_admission(
            vectors::SOURCE,
            Some("default"),
            Some(&id),
            &mut unlimited(),
        )
        .unwrap();
    assert!(transaction.stage(prepared, &mut unlimited()).is_err());
    assert_eq!(fixture.old_rows(), old);
    assert_eq!(fixture.packet_rows(), packets);
    assert_eq!(fixture.state_rows(), states);
}

#[test]
fn native_admission_model_ignored_or_aborted_inserts_rollback_both_new_rows() {
    for table in ["jj_native_admissions", "jj_native_admission_states"] {
        for fault in ["RAISE(IGNORE)", "RAISE(ABORT,'fixture')"] {
            let fixture = Fixture::new();
            fixture
                .conn()
                .execute_batch(&format!(
                    "CREATE TRIGGER fail_insert BEFORE INSERT ON {table} BEGIN SELECT {fault}; END;"
                ))
                .unwrap();
            reject_stage(&fixture, "left");
        }
    }
}

#[test]
fn native_admission_model_ignored_or_aborted_state_update_preserves_prior_cursor() {
    for fault in ["RAISE(IGNORE)", "RAISE(ABORT,'fixture')"] {
        let fixture = Fixture::new();
        committed(&fixture, "left");
        fixture.conn().execute_batch(&format!("CREATE TRIGGER fail_update BEFORE UPDATE ON jj_native_admission_states BEGIN SELECT {fault}; END;")).unwrap();
        reject_stage(&fixture, "merge");
    }
}

#[test]
fn native_admission_model_after_insert_deletion_or_rewrite_is_detected_before_commit() {
    for (table, body) in [
        (
            "jj_native_admissions",
            "DELETE FROM jj_native_admissions WHERE source_id=NEW.source_id;",
        ),
        (
            "jj_native_admissions",
            "UPDATE jj_native_admissions SET record=X'01' WHERE source_id=NEW.source_id;",
        ),
        (
            "jj_native_admission_states",
            "DELETE FROM jj_native_admission_states WHERE source_id=NEW.source_id;",
        ),
        (
            "jj_native_admission_states",
            "DELETE FROM jj_native_admission_states WHERE source_id=NEW.source_id; DELETE FROM jj_native_admissions WHERE source_id=NEW.source_id;",
        ),
        (
            "jj_native_admission_states",
            "UPDATE jj_native_admission_states SET state=X'01' WHERE source_id=NEW.source_id;",
        ),
        (
            "jj_native_admission_states",
            "UPDATE jj_native_registrations SET record=X'01' WHERE source_id=NEW.source_id;",
        ),
    ] {
        let fixture = Fixture::new();
        fixture
            .conn()
            .execute_batch(&format!(
                "CREATE TRIGGER alter_insert AFTER INSERT ON {table} BEGIN {body} END;"
            ))
            .unwrap();
        reject_stage(&fixture, "left");
    }
}

#[test]
fn native_admission_model_valid_old_state_rewrite_is_not_mistaken_for_successful_cas() {
    let fixture = Fixture::new();
    committed(&fixture, "left");
    let state = vector("left_state");
    let raw = state
        .raw
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fixture.conn().execute_batch(&format!("CREATE TRIGGER rewind AFTER UPDATE ON jj_native_admission_states WHEN NEW.admission_id='{}' BEGIN
       UPDATE jj_native_admission_states SET admission_id='{}',state=X'{}',checksum='{}' WHERE source_id=NEW.source_id;
       END;", vector("merge").admission_id, state.admission_id, raw, state.checksum)).unwrap();
    reject_stage(&fixture, "merge");
}

#[test]
fn native_admission_model_valid_future_trigger_packet_cannot_escape_readback() {
    let fixture = Fixture::new();
    let packet = vector("merge");
    let raw = packet
        .raw
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fixture
        .conn()
        .execute_batch(&format!(
            "CREATE TRIGGER add_future AFTER INSERT ON jj_native_admission_states BEGIN
      INSERT INTO jj_native_admissions VALUES (NEW.source_id,'{}',2,X'{}','{}'); END;",
            packet.admission_id, raw, packet.checksum
        ))
        .unwrap();
    reject_stage(&fixture, "left");
}

#[test]
fn native_admission_model_exact_receipt_retry_executes_no_insert_or_update_trigger() {
    let fixture = Fixture::new();
    fixture.seed("merge");
    fixture.conn().execute_batch("CREATE TRIGGER reject_insert BEFORE INSERT ON jj_native_admissions BEGIN SELECT RAISE(ABORT,'unexpected insert'); END;
      CREATE TRIGGER reject_update BEFORE UPDATE ON jj_native_admission_states BEGIN SELECT RAISE(ABORT,'unexpected update'); END;").unwrap();
    let input = Input::from("left");
    let prepared = input.prepare().unwrap();
    let id = prepared.admission_id().to_owned();
    let mut journal = fixture.journal();
    let transaction = journal
        .begin_native_admission(vectors::SOURCE, None, Some(&id), &mut unlimited())
        .unwrap();
    let (token, outcome, saved) = transaction
        .stage(prepared, &mut unlimited())
        .unwrap()
        .into_parts();
    assert_eq!(outcome, NativeAdmissionOutcome::AlreadyAdmitted);
    assert_cursor(&saved, 2, "merge");
    token.commit().unwrap();
}

#[test]
fn native_admission_model_preflight_exposes_native_invalid_prior_packets_before_any_stage() {
    for historical in [false, true] {
        let fixture = Fixture::new();
        fixture.insert("wrong_native");
        if historical {
            fixture.insert("merge");
            fixture.insert("merge_state");
        } else {
            fixture.insert("wrong_native_state");
        }
        let before = fixture.packet_rows();
        let states = fixture.state_rows();
        let mut journal = fixture.journal();
        let transaction = journal
            .begin_native_admission(
                vectors::SOURCE,
                None,
                Some(vector("wrong_native").admission_id),
                &mut unlimited(),
            )
            .unwrap();
        let prior = transaction.snapshot().requested().unwrap();
        assert_eq!(
            prior.record.operations,
            Input::from("wrong_native").operations
        );
        assert!(
            crate::operations::jj::evidence::verify_evidence(
                vectors::PROFILE,
                &prior.record.operations[0]
            )
            .is_err()
        );
        drop(transaction);
        assert_eq!(fixture.packet_rows(), before);
        assert_eq!(fixture.state_rows(), states);
    }
}

#[test]
fn native_admission_model_coherent_valid_replacement_must_equal_the_intended_readback() {
    let control = Fixture::new();
    control.insert("baseline_only");
    control.insert("baseline_only_state");
    assert_cursor(
        &read(&control.journal(), None, &mut unlimited()).unwrap(),
        1,
        "baseline_only",
    );
    let fixture = Fixture::new();
    let packet = vector("baseline_only");
    let state = vector("baseline_only_state");
    let packet_hex = packet
        .raw
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let state_hex = state
        .raw
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fixture.conn().execute_batch(&format!("CREATE TRIGGER replace_valid AFTER INSERT ON jj_native_admission_states WHEN NEW.admission_id='{}' BEGIN
      DELETE FROM jj_native_admission_states WHERE source_id=NEW.source_id;
      DELETE FROM jj_native_admissions WHERE source_id=NEW.source_id;
      INSERT INTO jj_native_admissions VALUES (NEW.source_id,'{}',1,X'{}','{}');
      INSERT INTO jj_native_admission_states VALUES (NEW.source_id,'{}',X'{}','{}');
      END;",vector("left").admission_id,packet.admission_id,packet_hex,packet.checksum,state.admission_id,state_hex,state.checksum)).unwrap();
    reject_stage(&fixture, "left");
}
