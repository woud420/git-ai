use super::*;

#[test]
fn jj_observer_intent_missing_load_does_not_create_store_or_parents() {
    let fixture = Fixture::new();
    let path = fixture.directory.path().join("missing/observer.db");
    assert!(load(&path).unwrap().is_none());
    assert!(!path.parent().unwrap().exists());
    assert!(load(&fixture.path).unwrap().is_none());
    assert!(!fixture.path.exists());
}

#[test]
fn jj_observer_intent_first_replace_and_reopen_preserve_target_and_block_state() {
    let fixture = Fixture::new();
    let first = record(1);
    replace(&fixture.path, &None, &first).unwrap();
    assert_loaded(&fixture.path, &first);
    let connection = fixture.connection();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    let mode: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert_eq!(mode, "wal");
    let mut blocked = record(2);
    blocked.blocked = Some(JjObserverError {
        code: "admission_unavailable".to_owned(),
        message: "saved native history unavailable".to_owned(),
        persisted: true,
    });
    replace(&fixture.path, &Some(first), &blocked).unwrap();
    assert_loaded(&fixture.path, &blocked);
    let mut disabled = record(3);
    disabled.enabled = false;
    replace(&fixture.path, &Some(blocked), &disabled).unwrap();
    assert_loaded(&fixture.path, &disabled);
    let before = snapshot(&connection);
    assert!(replace(&fixture.path, &None, &record(1)).is_err());
    assert_eq!(snapshot(&connection), before);
}

#[test]
fn jj_observer_intent_valid_empty_slot_can_be_filled_without_schema_changes() {
    let fixture = Fixture::new();
    let connection = fixture.manual(DDL, 1, true);
    assert!(load(&fixture.path).unwrap().is_none());
    let schema_before: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='jj_observer_intent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    replace(&fixture.path, &None, &record(1)).unwrap();
    assert_loaded(&fixture.path, &record(1));
    let schema_after: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='jj_observer_intent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_after, schema_before);
}

#[test]
fn jj_observer_intent_existing_empty_or_non_sqlite_file_is_not_adopted() {
    for bytes in [b"".as_slice(), b"not a SQLite database".as_slice()] {
        let fixture = Fixture::new();
        std::fs::write(&fixture.path, bytes).unwrap();
        assert!(load(&fixture.path).is_err());
        assert!(replace(&fixture.path, &None, &record(1)).is_err());
        assert_eq!(std::fs::read(&fixture.path).unwrap(), bytes);
    }
}

#[test]
fn jj_observer_intent_exact_old_payload_cas_checks_target_and_intent() {
    let fixture = Fixture::new();
    let connection = fixture.manual(DDL, 1, true);
    let original = record(1);
    raw_insert(
        &connection,
        1,
        &serde_json::to_vec_pretty(&original).unwrap(),
    );
    assert_loaded(&fixture.path, &original);
    let before = snapshot(&connection);
    let original_value = value(&original);
    let changes = [
        ("/target/journal_path_hex", json!(hex(b"/other/journal"))),
        (
            "/target/workspace_path_hex",
            json!(hex(b"/other/workspace")),
        ),
        ("/target/metadata/source_id", json!("4".repeat(64))),
        (
            "/target/metadata/initialization_receipt_id",
            json!("5".repeat(64)),
        ),
        ("/target/metadata/baseline_id", json!("6".repeat(64))),
        (
            "/target/metadata/workspace_name",
            json!("another workspace"),
        ),
        ("/target/metadata/attachment_id", json!("7".repeat(64))),
        ("/enabled", json!(false)),
        (
            "/blocked",
            json!({"code":"busy","message":"blocked","persisted":true}),
        ),
    ];
    for (pointer, replacement) in changes {
        let mut wrong = original_value.clone();
        *wrong.pointer_mut(pointer).unwrap() = replacement;
        let wrong: StoredIntent = serde_json::from_value(wrong).unwrap();
        assert!(
            replace(&fixture.path, &Some(wrong), &record(2)).is_err(),
            "{pointer}"
        );
        assert_eq!(snapshot(&connection), before, "{pointer}");
    }
    replace(&fixture.path, &Some(original), &record(2)).unwrap();
    assert_loaded(&fixture.path, &record(2));
}

#[test]
fn jj_observer_intent_revision_successor_and_overflow_refuse_without_changes() {
    for revision in [0, 2, i64::MAX as u64 + 1] {
        let fixture = Fixture::new();
        assert!(replace(&fixture.path, &None, &record(revision)).is_err());
        assert!(
            !fixture.path.exists(),
            "invalid new intent must fail before file creation"
        );
    }
    let fixture = Fixture::new();
    replace(&fixture.path, &None, &record(1)).unwrap();
    let connection = fixture.connection();
    let before = snapshot(&connection);
    for revision in [0, 1, 3, i64::MAX as u64 + 1] {
        assert!(replace(&fixture.path, &Some(record(1)), &record(revision)).is_err());
        assert_eq!(snapshot(&connection), before);
    }
    let maximum = Fixture::new();
    let connection = maximum.manual(DDL, 1, true);
    raw_insert(&connection, i64::MAX, &encoded(&record(i64::MAX as u64)));
    assert_loaded(&maximum.path, &record(i64::MAX as u64));
    let before = snapshot(&connection);
    assert!(
        replace(
            &maximum.path,
            &Some(record(i64::MAX as u64)),
            &record(i64::MAX as u64 + 1)
        )
        .is_err()
    );
    assert_eq!(snapshot(&connection), before);
}

#[test]
fn jj_observer_intent_exact_old_payload_cas_includes_block_error_contents() {
    let fixture = Fixture::new();
    let mut original = record(1);
    original.blocked = Some(JjObserverError {
        code: "unavailable".to_owned(),
        message: "original diagnostic".to_owned(),
        persisted: true,
    });
    replace(&fixture.path, &None, &original).unwrap();
    let connection = fixture.connection();
    let before = snapshot(&connection);
    for (pointer, replacement) in [
        ("/blocked/code", json!("different")),
        ("/blocked/message", json!("different diagnostic")),
    ] {
        let mut wrong = value(&original);
        *wrong.pointer_mut(pointer).unwrap() = replacement;
        let wrong: StoredIntent = serde_json::from_value(wrong).unwrap();
        assert!(replace(&fixture.path, &Some(wrong), &record(2)).is_err());
        assert_eq!(snapshot(&connection), before);
    }
    replace(&fixture.path, &Some(original), &record(2)).unwrap();
    assert_loaded(&fixture.path, &record(2));
}
