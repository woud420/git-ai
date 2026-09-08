use super::*;

#[test]
fn jj_admission_schema_v3_refuses_preexisting_partial_or_complete_admission_tables() {
    for existing in [
        "CREATE TABLE jj_native_admissions (unexpected TEXT)".to_owned(),
        "CREATE TABLE jj_native_admission_states (unexpected TEXT)".to_owned(),
        ADMISSIONS.to_owned(),
        STATES.to_owned(),
        format!("{ADMISSIONS}\n{STATES}"),
    ] {
        let fixture = frozen_v3();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(&existing).unwrap();
        reject_unchanged(&fixture);
        assert_version(&conn, "3");
    }
}

fn original(version: u8) -> Fixture {
    match version {
        1 => v1_fixture(),
        2 => frozen_v2(),
        3 => frozen_v3(),
        _ => panic!("unknown fixture version"),
    }
}

#[test]
fn jj_admission_schema_aborted_version_update_rolls_back_the_entire_upgrade_chain() {
    for version in [1, 2, 3] {
        for timing in ["BEFORE", "AFTER"] {
            let fixture = original(version);
            let conn = open_with_memory_limits(&fixture.path).unwrap();
            conn.execute_batch(&format!(
                "CREATE TRIGGER reject_admission_upgrade {timing} UPDATE ON schema_metadata
                 WHEN OLD.key='version' AND OLD.value='3' AND NEW.value='4'
                  AND (SELECT count(*) FROM sqlite_master WHERE type='table'
                       AND name IN ('jj_native_admissions','jj_native_admission_states'))=2
                 BEGIN SELECT RAISE(ABORT, 'injected v4 failure'); END;"
            ))
            .unwrap();
            reject_unchanged(&fixture);
            assert_version(&conn, &version.to_string());
            conn.execute_batch("DROP TRIGGER reject_admission_upgrade")
                .unwrap();
            drop(fixture.open());
            shape(&conn);
            empty(&conn);
        }
    }
}

#[test]
fn jj_admission_schema_ignored_update_cannot_commit_new_tables() {
    for version in [1, 2, 3] {
        let fixture = original(version);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER ignore_admission_upgrade BEFORE UPDATE ON schema_metadata
             WHEN OLD.key='version' AND OLD.value='3' AND NEW.value='4'
             BEGIN SELECT RAISE(IGNORE); END;",
        )
        .unwrap();
        reject_unchanged(&fixture);
        assert_version(&conn, &version.to_string());
        conn.execute_batch("DROP TRIGGER ignore_admission_upgrade")
            .unwrap();
        drop(fixture.open());
        shape(&conn);
    }
}

#[test]
fn jj_admission_schema_rewritten_update_cannot_publish_wrong_version() {
    for version in [1, 2, 3] {
        for replacement in ["1", "2", "3", "999", "04"] {
            let fixture = original(version);
            let conn = open_with_memory_limits(&fixture.path).unwrap();
            conn.execute_batch(&format!(
                "CREATE TRIGGER rewrite_admission_upgrade AFTER UPDATE ON schema_metadata
                 WHEN NEW.key='version' AND NEW.value='4'
                 BEGIN UPDATE schema_metadata SET value='{replacement}' WHERE key='version'; END;"
            ))
            .unwrap();
            reject_unchanged(&fixture);
            assert_version(&conn, &version.to_string());
        }
    }
}

#[test]
fn jj_admission_schema_existing_duplicate_version_rows_are_rejected() {
    for second in ["4", "999"] {
        let fixture = manual_v4(ADMISSIONS, STATES);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(
            "DROP TABLE schema_metadata;
             CREATE TABLE schema_metadata (key TEXT NOT NULL, value TEXT NOT NULL);
             INSERT INTO schema_metadata VALUES ('version','4');",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO schema_metadata VALUES ('version',?1)",
            [second],
        )
        .unwrap();
        reject_unchanged(&fixture);
    }
}

#[test]
fn jj_admission_schema_duplicate_version_after_update_rolls_back() {
    let fixture = frozen_v3();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(
        "DROP TABLE schema_metadata;
         CREATE TABLE schema_metadata (key TEXT NOT NULL, value TEXT NOT NULL);
         INSERT INTO schema_metadata VALUES ('version','3');
         CREATE TRIGGER duplicate_admission_version AFTER UPDATE ON schema_metadata
         WHEN NEW.key='version' AND NEW.value='4'
         BEGIN INSERT INTO schema_metadata VALUES ('version','4'); END;",
    )
    .unwrap();
    reject_unchanged(&fixture);
    assert_version(&conn, "3");
}

#[test]
fn jj_admission_schema_version_four_requires_exact_text_bytes() {
    for value in [
        Value::Text("04".to_owned()),
        Value::Text("4\n".to_owned()),
        Value::Text("4 ".to_owned()),
        Value::Text("+4".to_owned()),
        Value::Text("999".to_owned()),
        Value::Blob(b"4".to_vec()),
    ] {
        let fixture = manual_v4(ADMISSIONS, STATES);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute(
            "UPDATE schema_metadata SET value=?1 WHERE key='version'",
            [value],
        )
        .unwrap();
        reject_unchanged(&fixture);
    }
}
