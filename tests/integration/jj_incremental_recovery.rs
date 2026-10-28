use super::*;
use ciborium::Value;

#[test]
fn jj_incremental_recovery_rejects_rehashed_link_head_and_native_faults() {
    for fault in ["link", "heads", "native"] {
        run_case(
            &format!("history:admission:incremental:recovery_{fault}"),
            true,
            Policy::Root,
        );
    }
}

#[test]
fn jj_incremental_recovery_discards_interrupted_proof_and_rejects_database_replacement() {
    for fault in ["interrupted", "replacement"] {
        run_case(
            &format!("history:admission:incremental:recovery_{fault}"),
            false,
            Policy::Root,
        );
    }
}

fn open(case: &Case, config: &Config) -> JjObservationSession {
    JjObservationSession::new(
        case.open(),
        case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap()
}

fn finish(session: &mut JjObservationSession, config: &Config) {
    for _ in 0..8 {
        match session
            .replay_step(config, deadline(), &mut admission_budget())
            .unwrap()
        {
            JjObservationStep::Replaying => assert!(session.verified_cursor().is_none()),
            JjObservationStep::Unchanged => return,
            JjObservationStep::Admitted(_) => panic!("replay wrote new evidence"),
        }
    }
    panic!("replay did not finish");
}

fn field<'a>(value: &'a mut Value, name: &str) -> &'a mut Value {
    &mut value
        .as_map_mut()
        .unwrap()
        .iter_mut()
        .find(|(key, _)| key.as_text() == Some(name))
        .unwrap()
        .1
}

fn encode(value: &Value) -> Vec<u8> {
    let mut raw = Vec::new();
    ciborium::into_writer(value, &mut raw).unwrap();
    raw
}

pub(super) fn run(case: &Case, config: &Config) {
    let (journal, saved, _) = initial(case, config);
    let mut session = JjObservationSession::new(
        journal,
        case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap();
    let records = [native::rich_parent(), native::rich_child()];
    for record in &records {
        select(
            case,
            std::slice::from_ref(record),
            &[&record.operation_id],
            None,
        );
        assert!(matches!(
            session
                .step(config, deadline(), &mut admission_budget())
                .unwrap(),
            JjObservationStep::Admitted(_)
        ));
    }
    let cursor = session.verified_cursor().unwrap().clone();
    drop(session);
    let mut session = open(case, config);
    // Stop after finding a dependency; a replacement session must start untrusted.
    assert!(matches!(
        session
            .replay_step(config, deadline(), &mut admission_budget())
            .unwrap(),
        JjObservationStep::Replaying
    ));
    assert!(session.verified_cursor().is_none());
    let fault = case.name.rsplit(':').next().unwrap();
    if fault == "recovery_replacement" {
        let clone = case.journal_path.with_extension("clone");
        let moved = case.journal_path.with_extension("original");
        case.sql()
            .execute("VACUUM INTO ?1", [clone.to_str().unwrap()])
            .unwrap();
        fs::rename(&case.journal_path, &moved).unwrap();
        fs::rename(&clone, &case.journal_path).unwrap();
        let result = session.replay_step(config, deadline(), &mut admission_budget());
        // Restore the original before any connection reopens its WAL sidecars.
        fs::rename(&case.journal_path, &clone).unwrap();
        fs::rename(&moved, &case.journal_path).unwrap();
        assert!(result.is_err());
        assert!(session.verified_cursor().is_none());
    } else if fault == "recovery_interrupted" {
        drop(session);
        session = open(case, config);
        assert!(session.verified_cursor().is_none());
        // Reach Ready, then change an old dependency without changing the cursor.
        finish(&mut session, config);
        let conn = case.sql();
        conn.execute(
            "UPDATE jj_native_admissions SET checksum='corrupted' WHERE generation=1",
            [],
        )
        .unwrap();
        assert!(
            session
                .read_latest_segment(config, deadline(), &mut admission_budget())
                .unwrap()
                .is_none()
        );
        assert!(session.verified_cursor().is_none());
        assert!(matches!(
            session
                .replay_step(config, deadline(), &mut admission_budget())
                .unwrap(),
            JjObservationStep::Replaying
        ));
        assert!(
            session
                .replay_step(config, deadline(), &mut admission_budget())
                .is_err()
        );
        conn.execute(
            "UPDATE jj_native_admissions SET checksum=admission_id WHERE generation=1",
            [],
        )
        .unwrap();
    } else {
        drop(session);
        let conn = case.sql();
        let (old_id, old_raw): (String, Vec<u8>) = conn
            .query_row(
                "SELECT admission_id,record FROM jj_native_admissions WHERE generation=2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let old_state: Vec<u8> = conn
            .query_row("SELECT state FROM jj_native_admission_states", [], |row| {
                row.get(0)
            })
            .unwrap();
        let mut packet: Value = ciborium::from_reader(old_raw.as_slice()).unwrap();
        match fault {
            "recovery_link" => {
                *field(&mut packet, "previous_admission_id") = Value::Text("ab".repeat(32))
            }
            "recovery_heads" => {
                *field(&mut packet, "expected_admitted_head_ids") =
                    Value::Array(vec![Value::Text(MERGE_ID.to_owned())])
            }
            "recovery_native" => {
                let operation = &mut field(&mut packet, "operations").as_array_mut().unwrap()[0];
                let Value::Bytes(bytes) = field(operation, "operation_bytes") else {
                    panic!("native bytes absent")
                };
                bytes[0] ^= 1;
            }
            _ => panic!("unknown recovery fault"),
        }
        let raw = encode(&packet);
        let id = hash(&raw);
        let mut state: Value = ciborium::from_reader(old_state.as_slice()).unwrap();
        *field(&mut state, "admission_id") = Value::Text(id.clone());
        let state = encode(&state);
        conn.execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE")
            .unwrap();
        conn.execute("UPDATE jj_native_admissions SET admission_id=?1,record=?2,checksum=?1 WHERE generation=2", rusqlite::params![id, raw]).unwrap();
        conn.execute(
            "UPDATE jj_native_admission_states SET admission_id=?1,state=?2,checksum=?3",
            rusqlite::params![id, state, hash(&state)],
        )
        .unwrap();
        conn.execute_batch("COMMIT").unwrap();
        let corrupted = admission_rows(case);
        session = open(case, config); // Structural hashes are valid; native replay must reject.
        let mut rejected = false;
        for _ in 0..8 {
            match session.replay_step(config, deadline(), &mut admission_budget()) {
                Ok(JjObservationStep::Replaying) => assert!(session.verified_cursor().is_none()),
                Err(_) => {
                    rejected = true;
                    break;
                }
                other => panic!("invalid evidence became ready: {other:?}"),
            }
        }
        assert!(rejected);
        assert!(session.verified_cursor().is_none());
        assert_eq!(admission_rows(case), corrupted);
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        conn.execute("UPDATE jj_native_admissions SET admission_id=?1,record=?2,checksum=?1 WHERE generation=2", rusqlite::params![old_id, old_raw]).unwrap();
        conn.execute(
            "UPDATE jj_native_admission_states SET admission_id=?1,state=?2,checksum=?3",
            rusqlite::params![old_id, old_state, hash(&old_state)],
        )
        .unwrap();
        conn.execute_batch("COMMIT").unwrap();
    }
    drop(session);
    let before = admission_rows(case);
    let mut resumed = open(case, config);
    assert!(resumed.verified_cursor().is_none());
    finish(&mut resumed, config);
    assert_eq!(resumed.verified_cursor(), Some(&cursor));
    assert_eq!(admission_rows(case), before);
    assert_eq!(
        cursor.baseline_id(),
        saved.baseline().receipt().baseline_id()
    );
    assert!(
        case.open()
            .status(saved.source_id())
            .unwrap()
            .applied_heads
            .is_empty()
    );
}
