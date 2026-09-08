use super::*;
use ciborium::Value;

#[test]
fn jj_baseline_persistence_second_insert_failure_rolls_back_both_native_rows() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_native_source BEFORE INSERT ON jj_native_sources BEGIN SELECT RAISE(FAIL, 'injected native source failure'); END").unwrap();
    let error = rejected(
        install(&mut journal, &fixture.source, &[first()]),
        "injected native source failure",
    );
    assert!(
        std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<JournalError>()
            .is_some()
    );
    assert_eq!(native_counts(&fixture), (0, 0));
    assert!(reopen(&journal, &fixture.source).unwrap().is_none());
    assert_eq!(journal.status(&fixture.source).unwrap().generation, 0);
    conn.execute_batch("DROP TRIGGER reject_native_source")
        .unwrap();
    drop(journal);
    let mut journal = fixture.open();
    installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    assert_eq!(native_counts(&fixture), (1, 1));
}

#[test]
fn jj_baseline_persistence_orphan_record_prevents_absent_result_or_reinstallation() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "DELETE FROM jj_native_sources WHERE source_id = ?1",
        [&fixture.source],
    )
    .unwrap();
    let before = native_rows(&fixture);
    rejected(reopen(&journal, &fixture.source), "gap");
    rejected(install(&mut journal, &fixture.source, &[first()]), "gap");
    rejected(install(&mut journal, &fixture.source, &[left()]), "gap");
    assert_eq!(native_rows(&fixture), before);
    assert_eq!(native_counts(&fixture), (1, 0));
}

#[test]
fn jj_baseline_persistence_missing_anchor_record_does_not_fall_back_to_opaque_capture() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    journal
        .capture(&fixture.batch(FIRST_ID, vec![first()]))
        .unwrap();
    installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    conn.execute(
        "DELETE FROM jj_native_baselines WHERE source_id = ?1",
        [&fixture.source],
    )
    .unwrap();
    let before = native_rows(&fixture);
    rejected(reopen(&journal, &fixture.source), "baseline");
    rejected(
        install(&mut journal, &fixture.source, &[first()]),
        "baseline",
    );
    assert_eq!(native_rows(&fixture), before);
    assert_eq!(journal.pending(&fixture.source, 8).unwrap(), [first()]);
    assert_eq!(native_counts(&fixture), (0, 1));
}

#[test]
fn jj_baseline_persistence_extra_source_local_record_is_not_hidden_by_valid_active_pointer() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute("INSERT INTO jj_native_baselines(source_id, baseline_id, record, checksum) SELECT source_id, ?1, record, checksum FROM jj_native_baselines WHERE source_id = ?2", params!["ff".repeat(32), fixture.source]).unwrap();
    let before = native_rows(&fixture);
    rejected(reopen(&journal, &fixture.source), "baseline");
    rejected(
        install(&mut journal, &fixture.source, &[first()]),
        "baseline",
    );
    assert_eq!(native_rows(&fixture), before);
}

#[test]
fn jj_baseline_persistence_corrupt_checksums_reject_reopen_and_retry_without_refunding_reads() {
    for state in [true, false] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        let selected = if state {
            conn.query_row(
                "SELECT length(state) FROM jj_native_sources WHERE source_id = ?1",
                [&fixture.source],
                |row| row.get::<_, usize>(0),
            )
            .unwrap()
        } else {
            stored_bytes(&fixture)
        };
        let sql = if state {
            "UPDATE jj_native_sources SET checksum = ?1 WHERE source_id = ?2"
        } else {
            "UPDATE jj_native_baselines SET checksum = ?1 WHERE source_id = ?2"
        };
        conn.execute(sql, params!["00".repeat(32), fixture.source])
            .unwrap();
        let before = native_rows(&fixture);
        let mut budget = full_budget();
        rejected(
            reopen_current_state_baseline(&journal, &fixture.source, &mut budget),
            "checksum",
        );
        assert_eq!(budget.consumed(), selected);
        rejected(
            install(&mut journal, &fixture.source, &[first()]),
            "checksum",
        );
        assert_eq!(native_rows(&fixture), before);
    }
}

#[test]
fn jj_baseline_persistence_record_digest_must_match_key_even_with_consistent_state_pointer() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    let wrong = "ff".repeat(32);
    conn.execute(
        "UPDATE jj_native_baselines SET baseline_id = ?1 WHERE source_id = ?2",
        params![wrong, fixture.source],
    )
    .unwrap();
    conn.execute(
        "UPDATE jj_native_sources SET baseline_id = ?1 WHERE source_id = ?2",
        params![wrong, fixture.source],
    )
    .unwrap();
    mutate_state(&fixture, |state| {
        *field(state, "baseline_id") = Value::Text(wrong)
    });
    let before = native_rows(&fixture);
    rejected(reopen(&journal, &fixture.source), "digest");
    rejected(install(&mut journal, &fixture.source, &[first()]), "digest");
    assert_eq!(native_rows(&fixture), before);
}

#[test]
fn jj_baseline_persistence_reopen_revalidates_native_evidence_after_all_storage_hashes_repaired() {
    for fault in ["parent", "operation", "view"] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
        mutate_record_and_rekey(&fixture, |record| {
            let Value::Array(anchors) = field(record, "anchors") else {
                panic!("expected anchors")
            };
            assert_eq!(anchors.len(), 1);
            let anchor = &mut anchors[0];
            match fault {
                "parent" => {
                    *field(anchor, "parent_ids") = Value::Array(vec![Value::Text("ee".repeat(64))])
                }
                "operation" => *field(anchor, "operation_bytes") = bytes_value(&unhex(LEFT_HEX)),
                "view" => {
                    *field(anchor, "view_bytes") =
                        bytes_value(&unhex(crate::jj_view::vectors::RICH_HEX))
                }
                _ => unreachable!(),
            }
        });
        let before = native_rows(&fixture);
        let mut budget = full_budget();
        let category = if fault == "parent" { "parent" } else { "hash" };
        let error = rejected(
            reopen_current_state_baseline(&journal, &fixture.source, &mut budget),
            category,
        );
        assert!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<JjBaselineError>()
                .is_some()
        );
        assert_eq!(budget.consumed(), stored_bytes(&fixture));
        assert_eq!(native_rows(&fixture), before);
    }
}

#[test]
fn jj_baseline_persistence_checks_state_source_profile_generation_and_head_relationships() {
    for target in [
        "source_id",
        "reader_profile",
        "generation",
        "captured_head_ids",
    ] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
        mutate_state(&fixture, |state| {
            *field(state, target) = match target {
                "source_id" => Value::Text("02".repeat(32)),
                "reader_profile" => Value::Text("jj-simple-op-store/0.45.0".to_owned()),
                "generation" => Value::Integer(2.into()),
                "captured_head_ids" => Value::Array(vec![Value::Text(LEFT_ID.to_owned())]),
                _ => unreachable!(),
            };
        });
        let before = native_rows(&fixture);
        rejected(reopen(&journal, &fixture.source), "baseline");
        rejected(
            install(&mut journal, &fixture.source, &[first()]),
            "baseline",
        );
        assert_eq!(native_rows(&fixture), before);
    }
}

fn bytes_value(bytes: &[u8]) -> Value {
    Value::Array(
        bytes
            .iter()
            .map(|byte| Value::Integer((*byte).into()))
            .collect(),
    )
}

#[test]
fn jj_baseline_persistence_ignored_insert_cannot_acknowledge_a_missing_baseline() {
    for table in ["jj_native_baselines", "jj_native_sources"] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TRIGGER ignore_native_insert BEFORE INSERT ON {table}
             BEGIN SELECT RAISE(IGNORE); END;"
        ))
        .unwrap();
        assert!(install(&mut journal, &fixture.source, &[first()]).is_err());
        assert_eq!(native_counts(&fixture), (0, 0));
        assert!(reopen(&journal, &fixture.source).unwrap().is_none());
        conn.execute_batch("DROP TRIGGER ignore_native_insert")
            .unwrap();
        installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
        assert_eq!(native_counts(&fixture), (1, 1));
    }
}

#[test]
fn jj_baseline_persistence_after_insert_changes_cannot_acknowledge_invalid_native_state() {
    for mutation in [
        "DELETE FROM jj_native_sources WHERE source_id = NEW.source_id;",
        "UPDATE jj_native_sources SET checksum = 'invalid' WHERE source_id = NEW.source_id;",
    ] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TRIGGER change_native_state AFTER INSERT ON jj_native_sources
             BEGIN {mutation} END;"
        ))
        .unwrap();
        assert!(install(&mut journal, &fixture.source, &[first()]).is_err());
        assert_eq!(native_counts(&fixture), (0, 0));
        assert!(reopen(&journal, &fixture.source).unwrap().is_none());
        conn.execute_batch("DROP TRIGGER change_native_state")
            .unwrap();
        installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
        assert_eq!(native_counts(&fixture), (1, 1));
    }
}
