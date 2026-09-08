use super::*;
use ciborium::Value;
use sha2::{Digest, Sha256};

fn after_stage(mutate: impl Fn(&Case), with_prior: bool) {
    let (case, mut journal, zero) = Case::new();
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    case.select(&records, &records[0]);
    let expected = if with_prior {
        admitted(run(&case, &mut journal, &zero, &mut Hook::new(|_| {})).unwrap())
            .current_cursor()
            .clone()
    } else {
        zero
    };
    case.select(&records, &records[1]);
    let before = sql_snapshot(&case.db, true);
    let counts = admission_counts(&case.db);
    let mut injected_files = None;
    let mut hooks = Hook::new(|phase| {
        if phase == AdmissionPhase::ReadbackVerified {
            assert_eq!(admission_counts(&case.db), counts);
            mutate(&case);
            injected_files = Some(filesystem(&case.fixture));
        }
    });
    refuse(run(&case, &mut journal, &expected, &mut hooks));
    assert_eq!(hooks.phases, ALL_PHASES);
    drop(hooks);
    assert!(
        sql_snapshot(&case.db, true) == before,
        "staged SQL escaped rollback"
    );
    assert!(filesystem(&case.fixture) == injected_files.unwrap());
    assert_eq!(admission_counts(&case.db), counts);
}

#[test]
fn admission_coordinator_late_seal_substitution_rolls_back_uncommitted_rows() {
    after_stage(
        |case| {
            let seal = case.fixture.repo.join("git-ai/registration");
            let raw = fs::read(&seal).unwrap();
            let old = fs::metadata(&seal).unwrap().ino();
            fs::rename(&seal, seal.with_extension("retained")).unwrap();
            fs::write(&seal, raw).unwrap();
            fs::set_permissions(&seal, fs::Permissions::from_mode(0o600)).unwrap();
            assert_ne!(fs::metadata(&seal).unwrap().ino(), old);
        },
        false,
    );
}

#[test]
fn admission_coordinator_late_ancestor_replacement_rolls_back_uncommitted_rows() {
    after_stage(
        |case| {
            let path = &case.fixture.ancestor;
            fs::rename(path, path.with_extension("retained")).unwrap();
            fs::create_dir(path).unwrap();
        },
        false,
    );
}

#[test]
fn admission_coordinator_late_backend_and_pointer_changes_roll_back() {
    for (relative, replacement) in [
        ("store/type", "future_git"),
        ("op_store/type", "future_op_store"),
        ("op_heads/type", "future_op_heads"),
        ("store/git_target", "../../../.git/."),
    ] {
        after_stage(
            |case| fs::write(case.fixture.repo.join(relative), replacement).unwrap(),
            false,
        );
    }
}

#[test]
fn admission_coordinator_late_git_head_change_preserves_prior_generation() {
    after_stage(
        |case| {
            fs::write(
                case.fixture.root.join(".git/HEAD"),
                b"ref: refs/heads/changed\n",
            )
            .unwrap();
        },
        true,
    );
}

#[test]
fn admission_coordinator_deadline_at_each_phase_never_commits_or_advances() {
    for (index, phase) in ALL_PHASES.into_iter().enumerate() {
        let (case, mut journal, zero) = Case::new();
        let record = fixtures::rich_parent();
        case.select(std::slice::from_ref(&record), &record);
        let before = sql_snapshot(&case.db, true);
        let files = filesystem(&case.fixture);
        let mut hooks = Hook::new(|at| {
            if at == AdmissionPhase::ReadbackVerified {
                assert_eq!(admission_counts(&case.db), [0, 0]);
            }
        });
        hooks.expires = Some(phase);
        let error = refuse(run(&case, &mut journal, &zero, &mut hooks));
        assert!(error.to_string().contains("deadline"), "{error}");
        assert_eq!(hooks.phases, ALL_PHASES[..=index]);
        assert!(sql_snapshot(&case.db, true) == before);
        assert!(filesystem(&case.fixture) == files);
    }
}

fn field_mut<'a>(value: &'a mut Value, name: &str) -> &'a mut Value {
    let Value::Map(fields) = value else {
        panic!("expected canonical map")
    };
    &mut fields
        .iter_mut()
        .find(|(key, _)| key.as_text() == Some(name))
        .unwrap()
        .1
}

fn encode(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).unwrap();
    bytes
}

fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn corrupt_native_prior(path: &Path) {
    let conn = open_with_memory_limits(path).unwrap();
    let (old_id, raw): (String, Vec<u8>) = conn
        .query_row(
            "SELECT admission_id,record FROM jj_native_admissions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let mut value: Value = ciborium::from_reader(raw.as_slice()).unwrap();
    let Value::Array(operations) = field_mut(&mut value, "operations") else {
        panic!("expected operations")
    };
    let Value::Bytes(bytes) = field_mut(&mut operations[0], "operation_bytes") else {
        panic!("expected native bytes")
    };
    bytes[0] = 0;
    let raw = encode(&value);
    let id = checksum(&raw);
    assert_ne!(old_id, id);
    let state: Vec<u8> = conn
        .query_row("SELECT state FROM jj_native_admission_states", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut state: Value = ciborium::from_reader(state.as_slice()).unwrap();
    *field_mut(&mut state, "admission_id") = Value::Text(id.clone());
    let state = encode(&state);
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(conn.execute("UPDATE jj_native_admissions SET admission_id=?1,record=?2,checksum=?1 WHERE admission_id=?3", rusqlite::params![id,raw,old_id]).unwrap(), 1);
    assert_eq!(
        conn.execute(
            "UPDATE jj_native_admission_states SET admission_id=?1,state=?2,checksum=?3",
            rusqlite::params![id, state, checksum(&state)]
        )
        .unwrap(),
        1
    );
    conn.execute_batch("COMMIT").unwrap();
}

#[test]
fn admission_coordinator_new_valid_packet_cannot_hide_corrupted_prior_native_evidence() {
    let (case, mut journal, zero) = Case::new();
    let records = [fixtures::rich_parent(), fixtures::rich_child()];
    case.select(&records, &records[0]);
    let first = admitted(run(&case, &mut journal, &zero, &mut Hook::new(|_| {})).unwrap());
    case.select(&records, &records[1]);
    let mut changed_sql = None;
    let files = filesystem(&case.fixture);
    let mut hooks = Hook::new(|phase| {
        if phase == AdmissionPhase::HistoryCollected {
            corrupt_native_prior(&case.db);
            changed_sql = Some(sql_snapshot(&case.db, true));
        }
    });
    refuse(run(&case, &mut journal, first.current_cursor(), &mut hooks));
    assert_eq!(hooks.phases, [AdmissionPhase::HistoryCollected]);
    drop(hooks);
    assert!(sql_snapshot(&case.db, true) == changed_sql.unwrap());
    assert!(filesystem(&case.fixture) == files);
    let snapshot = journal
        .read_native_admission_snapshot(
            case.registered.source_id(),
            Some("default"),
            None,
            &mut reads(),
        )
        .unwrap();
    let prior = snapshot.latest.unwrap();
    assert_eq!(prior.generation, 1);
    assert_eq!(prior.record.operations.len(), 1);
    assert!(
        crate::operations::jj::evidence::verify_evidence(
            JJ_OBSERVATION_READER_PROFILE,
            &prior.record.operations[0],
        )
        .is_err()
    );
}
