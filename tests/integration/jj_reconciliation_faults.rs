use super::*;

pub(super) fn corrupt(case: &Case, config: &Config) {
    let (mut journal, saved, _zero, _first, second) = two(case, config);
    let target = SavedTarget::from_registered(&saved);
    let conn = case.sql();
    let kind = case.name.rsplit(':').next().unwrap();
    match kind {
        "state_checksum" => {
            assert_eq!(
                conn.execute(
                    "UPDATE jj_native_admission_states SET checksum=?1",
                    ["00".repeat(32)]
                )
                .unwrap(),
                1
            );
        }
        "packet_checksum" => {
            assert_eq!(
                conn.execute(
                    "UPDATE jj_native_admissions SET checksum=?1 WHERE admission_id=?2",
                    [
                        &"00".repeat(32),
                        second.admission().receipt().admission_id()
                    ]
                )
                .unwrap(),
                1
            );
        }
        "registration" => {
            assert_eq!(
                conn.execute(
                    "UPDATE jj_native_registrations SET checksum=?1",
                    ["00".repeat(32)]
                )
                .unwrap(),
                1
            );
        }
        "native_packet" | "scope" => {
            let old = second.admission().receipt().admission_id();
            let raw: Vec<u8> = conn
                .query_row(
                    "SELECT record FROM jj_native_admissions WHERE admission_id=?1",
                    [old],
                    |row| row.get(0),
                )
                .unwrap();
            let mut record = decoded(&raw);
            let raw_state: Vec<u8> = conn
                .query_row("SELECT state FROM jj_native_admission_states", [], |row| {
                    row.get(0)
                })
                .unwrap();
            let mut saved_state = decoded(&raw_state);
            if kind == "native_packet" {
                let Value::Array(operations) = field_mut(&mut record, "operations") else {
                    panic!("expected operations")
                };
                let Value::Bytes(bytes) = field_mut(&mut operations[0], "operation_bytes") else {
                    panic!("expected byte string")
                };
                assert!(!bytes.is_empty());
                bytes[0] = 0;
            } else {
                let other = Value::Text("ab".repeat(32));
                *field_mut(&mut record, "initialization_receipt_id") = other.clone();
                *field_mut(&mut saved_state, "initialization_receipt_id") = other;
            }
            let raw = encoded(&record);
            let id = hash(&raw);
            *field_mut(&mut saved_state, "admission_id") = Value::Text(id.clone());
            let bytes = encoded(&saved_state);
            conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
            assert_eq!(conn.execute("UPDATE jj_native_admissions SET admission_id=?1,record=?2,checksum=?1 WHERE admission_id=?3",
                rusqlite::params![id,raw,old]).unwrap(), 1);
            assert_eq!(
                conn.execute(
                    "UPDATE jj_native_admission_states SET admission_id=?1,state=?2,checksum=?3",
                    rusqlite::params![id, bytes, hash(&bytes)]
                )
                .unwrap(),
                1
            );
        }
        other => panic!("unknown reconciliation corruption {other}"),
    }
    drop(conn);
    capture_current_state(&case.context(), deadline()).unwrap();
    failed(checked_status(
        case,
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    ));
    refuse(case, &mut journal, config, &target, second.current_cursor());
}

pub(super) fn gap(case: &Case, config: &Config) {
    let (mut journal, saved, _zero, first, second) = two(case, config);
    let target = SavedTarget::from_registered(&saved);
    let conn = case.sql();
    conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let kind = case.name.rsplit(':').next().unwrap();
    match kind {
        "state_gap" => {
            assert_eq!(
                conn.execute("DELETE FROM jj_native_admission_states", [])
                    .unwrap(),
                1
            );
        }
        "packet_gap" => {
            assert_eq!(
                conn.execute("DELETE FROM jj_native_admissions", [])
                    .unwrap(),
                2
            );
        }
        "both_erased" => {
            conn.execute_batch(
                "DELETE FROM jj_native_admission_states; DELETE FROM jj_native_admissions;",
            )
            .unwrap();
        }
        "higher" => {
            let raw: Vec<u8> = conn
                .query_row("SELECT state FROM jj_native_admission_states", [], |row| {
                    row.get(0)
                })
                .unwrap();
            let mut value = decoded(&raw);
            *field_mut(&mut value, "admission_id") =
                Value::Text(first.admission().receipt().admission_id().to_owned());
            *field_mut(&mut value, "generation") = Value::Integer(1.into());
            *field_mut(&mut value, "admitted_head_ids") = Value::Array(
                first
                    .current_cursor()
                    .admitted_head_ids()
                    .iter()
                    .cloned()
                    .map(Value::Text)
                    .collect(),
            );
            let bytes = encoded(&value);
            assert_eq!(
                conn.execute(
                    "UPDATE jj_native_admission_states SET admission_id=?1,state=?2,checksum=?3",
                    rusqlite::params![
                        first.admission().receipt().admission_id(),
                        bytes,
                        hash(&bytes)
                    ]
                )
                .unwrap(),
                1
            );
            case.heads(&[&native::rich_parent().operation_id]);
        }
        "duplicate" => {
            conn.execute_batch("CREATE TABLE unindexed_admissions AS SELECT * FROM jj_native_admissions;
                DROP TABLE jj_native_admissions; ALTER TABLE unindexed_admissions RENAME TO jj_native_admissions;").unwrap();
            assert_eq!(conn.execute("INSERT INTO jj_native_admissions SELECT source_id,?1,generation,record,checksum FROM jj_native_admissions WHERE admission_id=?2",
                [&"cd".repeat(32), second.admission().receipt().admission_id()]).unwrap(), 1);
        }
        other => panic!("unknown reconciliation gap {other}"),
    }
    drop(conn);
    if kind == "both_erased" {
        let observed = status(case, &journal, config);
        assert_cursor(observed.cursor(), &saved, 0, &[MERGE_ID]);
    } else {
        failed(checked_status(
            case,
            &journal,
            &case.context(),
            config,
            deadline(),
            &mut admission_budget(),
        ));
    }
    let expected = if kind == "higher" {
        first.current_cursor()
    } else {
        second.current_cursor()
    };
    refuse(case, &mut journal, config, &target, expected);
}

pub(super) fn budget(case: &Case, config: &Config) {
    let (mut journal, saved, _zero, _first, second) = two(case, config);
    let target = SavedTarget::from_registered(&saved);
    let required = registration_bytes(case) + selected_bytes(case, None);
    let mut exact = ReadBudget::new(required);
    checks::unchanged(
        reconcile(
            case,
            &mut journal,
            config,
            target.with(second.current_cursor().expectation()),
            deadline(),
            &mut exact,
        )
        .unwrap(),
    );
    assert_eq!(exact.consumed(), required);
    assert_eq!(exact.remaining(), 0);
    failed(reconcile(
        case,
        &mut journal,
        config,
        target.with(second.current_cursor().expectation()),
        deadline(),
        &mut exact,
    ));
    assert_eq!(exact.consumed(), required);
    let mut short = ReadBudget::new(required - 1);
    failed(reconcile(
        case,
        &mut journal,
        config,
        target.with(second.current_cursor().expectation()),
        deadline(),
        &mut short,
    ));
    assert!(short.consumed() > 0 && short.consumed() < required);
}

pub(super) fn early(case: &Case, config: &Config) {
    if case.name.ends_with(":absent") {
        let mut journal = case.open();
        let source = "ab".repeat(32);
        let target = SavedTarget {
            workspace_name: "default".to_owned(),
            attachment_id: source.clone(),
        };
        let heads = [MERGE_ID.to_owned()];
        let expected = NativeAdmissionExpectation {
            source_id: &source,
            initialization_receipt_id: &source,
            baseline_id: &source,
            generation: 0,
            admitted_head_ids: &heads,
        };
        failed(reconcile(
            case,
            &mut journal,
            config,
            target.with(expected),
            deadline(),
            &mut admission_budget(),
        ));
        assert!(!case.repo_dir.join("git-ai").exists());
        assert!(admission_rows(case).iter().all(|(_, rows)| rows.is_empty()));
        return;
    }
    let (mut journal, saved, zero) = initial(case, config);
    let target = SavedTarget::from_registered(&saved);
    if case.name.ends_with(":deadline") {
        let mut reads = admission_budget();
        failed(reconcile(
            case,
            &mut journal,
            config,
            target.with(zero.expectation()),
            Instant::now() - Duration::from_secs(1),
            &mut reads,
        ));
        assert_eq!(reads.consumed(), 0);
        return;
    }
    let other = "ab".repeat(32);
    for kind in ["source", "receipt", "baseline", "generation"] {
        let mut expected = zero.expectation();
        match kind {
            "source" => expected.source_id = &other,
            "receipt" => expected.initialization_receipt_id = &other,
            "baseline" => expected.baseline_id = &other,
            "generation" => expected.generation = 1,
            _ => unreachable!(),
        }
        failed(reconcile(
            case,
            &mut journal,
            config,
            target.with(expected),
            deadline(),
            &mut admission_budget(),
        ));
    }
}

pub(super) fn save_policy_target(case: &Case, config: &Config) {
    let journal = case.open();
    let current = status(case, &journal, config);
    let target = SavedTarget::from_registered(current.registration());
    fs::write(
        case.test_home.join("reconciliation-policy-target.json"),
        serde_json::to_vec(&target).unwrap(),
    )
    .unwrap();
}

pub(super) fn policy(case: &Case, config: &Config) {
    let target: SavedTarget = serde_json::from_slice(&crate::debug_context::read_jj_fixture(
        &case.test_home.join("reconciliation-policy-target.json"),
        32 * 1024,
    ))
    .unwrap();
    let saved: serde_json::Value = serde_json::from_slice(
        &fs::read(case.test_home.join("admission-policy-saved.json")).unwrap(),
    )
    .unwrap();
    let text = |key: &str| saved.get(key).unwrap().as_str().unwrap();
    let heads: Vec<String> = serde_json::from_value(saved["heads"].clone()).unwrap();
    let expected = NativeAdmissionExpectation {
        source_id: text("source"),
        initialization_receipt_id: text("initialization"),
        baseline_id: text("baseline"),
        generation: 1,
        admitted_head_ids: &heads,
    };
    let mut journal = case.open();
    capture_current_state(&case.context(), deadline()).unwrap();
    failed(reconcile(
        case,
        &mut journal,
        config,
        target.with(expected),
        deadline(),
        &mut admission_budget(),
    ));
    assert_eq!(
        packet(case, &journal, text("source"), text("admission"))
            .receipt()
            .generation(),
        1
    );
}
