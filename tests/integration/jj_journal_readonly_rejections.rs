use super::*;

#[test]
fn jj_journal_readonly_missing_paths_do_not_create_files_or_parent_directories() {
    let fixture = Fixture::new();
    assert!(!fixture.path.exists());
    assert!(JjObservationJournal::open_read_only_at_path(&fixture.path).is_err());
    assert!(!fixture.path.exists());
    let parent = fixture
        .path
        .parent()
        .unwrap()
        .join("not-created")
        .join("nested");
    assert!(JjObservationJournal::open_read_only_at_path(&parent.join("journal.sqlite")).is_err());
    assert!(!parent.parent().unwrap().exists());
}

#[test]
fn jj_journal_readonly_rejects_special_sqlite_names_and_uri_options() {
    let fixture = manual_v4(ADMISSIONS, STATES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = readonly_snapshot(&conn);
    let uri = format!("file:{}", fixture.path.display());
    for path in [
        "",
        ":memory:",
        &uri,
        &format!("{uri}?immutable=1"),
        &format!("{uri}?mode=rwc"),
    ] {
        assert!(
            JjObservationJournal::open_read_only_at_path(Path::new(path)).is_err(),
            "accepted special SQLite path {path:?}"
        );
    }
    let missing = fixture
        .path
        .parent()
        .unwrap()
        .join("uri-must-not-create.sqlite");
    let uri = format!("file:{}?mode=rwc", missing.display());
    assert!(JjObservationJournal::open_read_only_at_path(Path::new(&uri)).is_err());
    assert!(!missing.exists());
    assert!(readonly_snapshot(&conn) == before);

    #[cfg(unix)]
    {
        // An absolute path containing this basename is an ordinary disk path.
        let literal = fixture.path.parent().unwrap().join("file:literal.sqlite");
        fs::copy(&fixture.path, &literal).unwrap();
        let journal = JjObservationJournal::open_read_only_at_path(&literal).unwrap();
        fixture.assert_only_first(&journal);
    }
}

#[test]
fn jj_journal_readonly_rejects_empty_non_sqlite_and_unrelated_databases() {
    for bytes in [b"".as_slice(), b"not a SQLite database".as_slice()] {
        let fixture = Fixture::new();
        fs::write(&fixture.path, bytes).unwrap();
        assert!(JjObservationJournal::open_read_only_at_path(&fixture.path).is_err());
        assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    }
    let fixture = Fixture::new();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(
        "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES ('preserve me');",
    )
    .unwrap();
    assert_readonly_rejected_unchanged(&fixture);
}

#[test]
fn jj_journal_readonly_rejects_old_versions_without_migrating_any_rows() {
    for sql in [FROZEN_V1, FROZEN_V2, FROZEN_V3] {
        let fixture = Fixture::new();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(sql).unwrap();
        let before = fs::read(&fixture.path).unwrap();
        assert_readonly_rejected_unchanged(&fixture);
        assert_eq!(fs::read(&fixture.path).unwrap(), before);
    }
}

#[test]
fn jj_journal_readonly_requires_one_exact_supported_version() {
    for value in [
        Value::Text("5".into()),
        Value::Text("04".into()),
        Value::Text("4 ".into()),
        Value::Blob(b"4".to_vec()),
        Value::Integer(4),
        Value::Null,
    ] {
        let fixture = manual_v4(ADMISSIONS, STATES);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        // No affinity or uniqueness masks malformed scalar/cardinality cases.
        conn.execute_batch("DROP TABLE schema_metadata; CREATE TABLE schema_metadata(key, value);")
            .unwrap();
        conn.execute(
            "INSERT INTO schema_metadata VALUES ('version', ?1)",
            [value],
        )
        .unwrap();
        assert_readonly_rejected_unchanged(&fixture);
    }
    for mutation in [
        "DELETE FROM schema_metadata",
        "DROP TABLE schema_metadata",
        "ALTER TABLE schema_metadata RENAME TO ignored_metadata; CREATE TABLE schema_metadata(key, value); INSERT INTO schema_metadata VALUES ('version', '4'), ('version', '4')",
    ] {
        let fixture = manual_v4(ADMISSIONS, STATES);
        open_with_memory_limits(&fixture.path)
            .unwrap()
            .execute_batch(mutation)
            .unwrap();
        assert_readonly_rejected_unchanged(&fixture);
    }
}

#[test]
fn jj_journal_readonly_reuses_schema_checks_across_all_record_families() {
    for mutation in [
        "ALTER TABLE jj_operations RENAME COLUMN payload TO wrong_payload",
        "ALTER TABLE jj_native_baselines ADD COLUMN unexpected TEXT",
        "ALTER TABLE jj_native_workspaces ADD COLUMN unexpected TEXT",
        "ALTER TABLE jj_native_admissions ADD COLUMN unexpected TEXT",
        "DROP TABLE jj_native_admission_states",
    ] {
        let fixture = manual_v4(ADMISSIONS, STATES);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute_batch(mutation).unwrap();
        assert_readonly_rejected_unchanged(&fixture);
    }
    let fixture = manual_v4(
        &ADMISSIONS.replace(
            "UNIQUE (source_id, generation)",
            "UNIQUE (generation, source_id)",
        ),
        STATES,
    );
    assert_readonly_rejected_unchanged(&fixture);
    let fixture = manual_v4(
        ADMISSIONS,
        &STATES.replace(
            "REFERENCES jj_native_admissions(source_id, admission_id)",
            "REFERENCES jj_native_baselines(source_id, baseline_id)",
        ),
    );
    assert_readonly_rejected_unchanged(&fixture);
}
