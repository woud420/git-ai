use super::*;

#[test]
fn jj_observer_intent_valid_maximum_fields_and_opaque_unix_paths_round_trip() {
    let fixture = Fixture::new();
    let mut saved = record(1);
    let mut path = vec![b'x'; 64 * 1024];
    path[0] = b'/';
    path[1] = 0xff;
    saved.target.journal_path_hex = hex(&path);
    path[1] = 0xfe;
    saved.target.workspace_path_hex = hex(&path);
    saved.target.metadata.workspace_name = "\0".repeat(16 * 1024);
    saved.blocked = Some(JjObserverError {
        code: "a".repeat(64),
        message: "🦀".repeat(1024),
        persisted: true,
    });
    assert!(encoded(&saved).len() < MAX_PAYLOAD);
    replace(&fixture.path, &None, &saved).unwrap();
    assert_loaded(&fixture.path, &saved);
}

#[test]
fn jj_observer_intent_invalid_metadata_refuses_before_new_file_creation_and_on_load() {
    let mut cases = vec![
        ("/schema_version", json!(0)),
        ("/schema_version", json!(2)),
        ("/revision", json!(0)),
        ("/revision", json!(i64::MAX as u64 + 1)),
        ("/target/metadata/reader_profile", json!("wrong-profile")),
        ("/target/metadata/baseline_generation", json!(0)),
        ("/target/metadata/baseline_generation", json!(2)),
        ("/target/metadata/workspace_name", json!("")),
        (
            "/target/metadata/workspace_name",
            json!("é".repeat(8192) + "x"),
        ),
    ];
    for field in [
        "source_id",
        "initialization_receipt_id",
        "baseline_id",
        "attachment_id",
    ] {
        let pointer = match field {
            "source_id" => "/target/metadata/source_id",
            "initialization_receipt_id" => "/target/metadata/initialization_receipt_id",
            "baseline_id" => "/target/metadata/baseline_id",
            _ => "/target/metadata/attachment_id",
        };
        cases.extend([
            (pointer, json!("a".repeat(63))),
            (pointer, json!("A".repeat(64))),
            (pointer, json!("g".repeat(64))),
        ]);
    }
    for (pointer, replacement) in cases {
        let mut malformed = value(&record(1));
        *malformed.pointer_mut(pointer).unwrap() = replacement;
        let candidate: StoredIntent = serde_json::from_value(malformed.clone()).unwrap();
        let fixture = Fixture::new();
        assert_validation(replace(&fixture.path, &None, &candidate).unwrap_err());
        assert!(!fixture.path.exists(), "{pointer}");
        refuse_raw(&serde_json::to_vec(&malformed).unwrap(), 1);
    }
}

#[test]
fn jj_observer_intent_locator_validation_is_on_decoded_bytes_and_both_fields() {
    let mut over = vec![b'x'; 64 * 1024 + 1];
    over[0] = b'/';
    let cases = [
        String::new(),
        "2".to_owned(),
        "2F".to_owned(),
        "zz".to_owned(),
        hex(b"relative/path"),
        hex(b"/nul\0path"),
        hex(&over),
    ];
    for pointer in ["/target/journal_path_hex", "/target/workspace_path_hex"] {
        for replacement in &cases {
            let mut malformed = value(&record(1));
            *malformed.pointer_mut(pointer).unwrap() = json!(replacement);
            let candidate: StoredIntent = serde_json::from_value(malformed.clone()).unwrap();
            let fixture = Fixture::new();
            assert_validation(replace(&fixture.path, &None, &candidate).unwrap_err());
            assert!(!fixture.path.exists(), "{pointer}");
            refuse_raw(&serde_json::to_vec(&malformed).unwrap(), 1);
        }
    }
}

#[test]
fn jj_observer_intent_block_error_bounds_and_disabled_invariant_are_checked() {
    let mut base = value(&record(1));
    base["blocked"] =
        json!({"code":"unavailable_1","message":"bounded diagnostic","persisted":true});
    let changes = [
        ("/blocked/code", json!("")),
        ("/blocked/code", json!("x".repeat(65))),
        ("/blocked/code", json!("Upper")),
        ("/blocked/code", json!("non-ascii-é")),
        ("/blocked/code", json!("invalid-code")),
        ("/blocked/message", json!("x".repeat(1025))),
        ("/blocked/message", json!("🦀".repeat(1024) + "x")),
        ("/blocked/persisted", json!(false)),
        ("/enabled", json!(false)),
    ];
    for (pointer, replacement) in changes {
        let mut malformed = base.clone();
        *malformed.pointer_mut(pointer).unwrap() = replacement;
        let candidate: StoredIntent = serde_json::from_value(malformed.clone()).unwrap();
        let fixture = Fixture::new();
        assert_validation(replace(&fixture.path, &None, &candidate).unwrap_err());
        assert!(!fixture.path.exists(), "{pointer}");
        refuse_raw(&serde_json::to_vec(&malformed).unwrap(), 1);
    }
}

#[test]
fn jj_observer_intent_raw_payload_type_and_outer_size_gates_preserve_rows() {
    for payload in [
        rusqlite::types::Value::Text(String::from_utf8(encoded(&record(1))).unwrap()),
        rusqlite::types::Value::Integer(1),
        rusqlite::types::Value::Real(1.5),
    ] {
        let fixture = Fixture::new();
        let connection = fixture.manual(DDL, 1, true);
        connection
            .execute("INSERT INTO jj_observer_intent VALUES(1,1,?1)", [payload])
            .unwrap();
        let before = snapshot(&connection);
        assert_validation(load(&fixture.path).unwrap_err());
        assert!(replace(&fixture.path, &Some(record(1)), &record(2)).is_err());
        assert_eq!(snapshot(&connection), before);
    }
    for length in [0, MAX_PAYLOAD, MAX_PAYLOAD + 1] {
        // The outer-cap cases are malformed JSON, not claimed valid maximum records.
        refuse_raw(&vec![b'x'; length], 1);
    }
    refuse_raw(b"{", 1);
    refuse_raw(&[0xff], 1);
    refuse_raw(b"null", 1);
    refuse_raw(b"[]", 1);
}

#[test]
fn jj_observer_intent_invalid_new_payload_does_not_change_existing_intent() {
    let fixture = Fixture::new();
    let original = record(1);
    replace(&fixture.path, &None, &original).unwrap();
    let connection = fixture.connection();
    let before = snapshot(&connection);
    let mut malformed = record(2);
    malformed.target.metadata.workspace_name.clear();
    assert_validation(replace(&fixture.path, &Some(original), &malformed).unwrap_err());
    assert_eq!(snapshot(&connection), before);
    assert_loaded(&fixture.path, &record(1));
}

#[test]
fn jj_observer_intent_unknown_and_duplicate_json_fields_are_rejected() {
    let original = value(&record(1));
    for pointer in ["", "/target", "/target/metadata"] {
        let mut malformed = original.clone();
        malformed
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_owned(), json!(true));
        refuse_raw(&serde_json::to_vec(&malformed).unwrap(), 1);
    }
    let mut blocked = original;
    blocked["blocked"] =
        json!({"code":"blocked","message":"reason","persisted":true,"unexpected":true});
    refuse_raw(&serde_json::to_vec(&blocked).unwrap(), 1);
    let canonical = String::from_utf8(encoded(&record(1))).unwrap();
    let duplicate = canonical.replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    );
    assert_ne!(duplicate, canonical);
    refuse_raw(duplicate.as_bytes(), 1);
}
