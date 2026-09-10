use super::*;

#[test]
fn jj_registration_schema_v2_rejects_preexisting_partial_or_complete_v3_tables() {
    for existing in [
        "CREATE TABLE jj_native_registrations (unexpected TEXT)".to_owned(),
        "CREATE TABLE jj_native_workspaces (unexpected TEXT)".to_owned(),
        REGISTRATIONS.to_owned(),
        WORKSPACES.to_owned(),
        format!("{REGISTRATIONS}\n{WORKSPACES}"),
    ] {
        let fixture = frozen_v2();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(&existing).unwrap();
        assert_rejected_unchanged(&fixture);
        assert_version(&conn, "2");
    }
}

#[test]
fn jj_registration_schema_aborted_v3_version_update_rolls_back_entire_upgrade_chain() {
    for original in [1, 2] {
        for timing in ["BEFORE", "AFTER"] {
            let fixture = if original == 1 {
                v1_fixture()
            } else {
                frozen_v2()
            };
            let conn = open_with_memory_limits(&fixture.path).unwrap();
            let before_opaque = opaque_snapshot(&conn);
            let before_native = (original == 2).then(|| native_snapshot(&conn));
            conn.execute_batch(&format!(
                "CREATE TRIGGER reject_registration_upgrade {timing} UPDATE ON schema_metadata
                 WHEN OLD.key = 'version' AND OLD.value = '2' AND NEW.value = '3'
                  AND (SELECT count(*) FROM sqlite_master WHERE type = 'table'
                       AND name IN ('jj_native_registrations', 'jj_native_workspaces')) = 2
                 BEGIN SELECT RAISE(ABORT, 'injected v3 migration failure'); END;"
            ))
            .unwrap();
            assert_rejected_unchanged(&fixture);
            assert_version(&conn, &original.to_string());
            conn.execute_batch("DROP TRIGGER reject_registration_upgrade")
                .unwrap();
            fixture.assert_only_first(&fixture.open());
            assert_latest_registration_shape(&conn);
            assert_registration_tables_empty(&conn);
            assert_eq!(opaque_snapshot(&conn), before_opaque);
            if let Some(before_native) = before_native {
                assert_eq!(native_snapshot(&conn), before_native);
            } else {
                assert_native_tables_empty(&conn);
            }
        }
    }
}

#[test]
fn jj_registration_schema_ignored_v3_update_cannot_commit_registration_or_native_ddl() {
    for original in [1, 2] {
        let fixture = if original == 1 {
            v1_fixture()
        } else {
            frozen_v2()
        };
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER ignore_registration_upgrade BEFORE UPDATE ON schema_metadata
             WHEN OLD.key = 'version' AND OLD.value = '2' AND NEW.value = '3'
             BEGIN SELECT RAISE(IGNORE); END;",
        )
        .unwrap();
        assert_rejected_unchanged(&fixture);
        assert_version(&conn, &original.to_string());
        conn.execute_batch("DROP TRIGGER ignore_registration_upgrade")
            .unwrap();
        drop(fixture.open());
        assert_latest_registration_shape(&conn);
        assert_registration_tables_empty(&conn);
    }
}

#[test]
fn jj_registration_schema_rewritten_v3_value_rolls_back_all_staged_migrations() {
    for original in [1, 2] {
        for replacement in ["1", "2", "999", "03"] {
            let fixture = if original == 1 {
                v1_fixture()
            } else {
                frozen_v2()
            };
            let conn = open_with_memory_limits(&fixture.path).unwrap();
            conn.execute_batch(&format!(
                "CREATE TRIGGER rewrite_registration_upgrade AFTER UPDATE ON schema_metadata
                 WHEN NEW.key = 'version' AND NEW.value = '3'
                 BEGIN UPDATE schema_metadata SET value = '{replacement}' WHERE key = 'version'; END;"
            )).unwrap();
            assert_rejected_unchanged(&fixture);
            assert_version(&conn, &original.to_string());
        }
    }
}

#[test]
fn jj_registration_schema_existing_duplicate_v3_version_rows_are_not_accepted() {
    for second in ["3", "999"] {
        let fixture = manual_v3(REGISTRATIONS, WORKSPACES);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(
            "DROP TABLE schema_metadata;
             CREATE TABLE schema_metadata (key TEXT NOT NULL, value TEXT NOT NULL);
             INSERT INTO schema_metadata VALUES ('version', '3');",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO schema_metadata VALUES ('version', ?1)",
            [second],
        )
        .unwrap();
        assert_rejected_unchanged(&fixture);
    }
}

#[test]
fn jj_registration_schema_duplicate_version_inserted_after_update_is_rolled_back() {
    let fixture = frozen_v2();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(
        "DROP TABLE schema_metadata;
         CREATE TABLE schema_metadata (key TEXT NOT NULL, value TEXT NOT NULL);
         INSERT INTO schema_metadata VALUES ('version', '2');
         CREATE TRIGGER duplicate_registration_version AFTER UPDATE ON schema_metadata
         WHEN NEW.key = 'version' AND NEW.value = '3'
         BEGIN INSERT INTO schema_metadata VALUES ('version', '3'); END;",
    )
    .unwrap();
    assert_rejected_unchanged(&fixture);
    assert_version(&conn, "2");
}

#[test]
fn jj_registration_schema_version_three_requires_exact_text_bytes() {
    for value in [
        Value::Text("03".to_owned()),
        Value::Text("3\n".to_owned()),
        Value::Text("3 ".to_owned()),
        Value::Text("+3".to_owned()),
        Value::Blob(b"3".to_vec()),
    ] {
        let fixture = manual_v3(REGISTRATIONS, WORKSPACES);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute(
            "UPDATE schema_metadata SET value = ?1 WHERE key = 'version'",
            [value],
        )
        .unwrap();
        assert_rejected_unchanged(&fixture);
    }
}
