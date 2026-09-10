use super::*;

#[test]
fn jj_journal_existing_missing_paths_do_not_create_files_or_directories() {
    let fixture = Fixture::new();
    assert!(!fixture.path.exists());
    assert!(JjObservationJournal::open_existing_at_path(&fixture.path).is_err());
    assert!(!fixture.path.exists());
    let parent = fixture
        .path
        .parent()
        .unwrap()
        .join("not-created")
        .join("nested");
    assert!(JjObservationJournal::open_existing_at_path(&parent.join("journal.sqlite")).is_err());
    assert!(!parent.parent().unwrap().exists());
}

#[test]
fn jj_journal_existing_special_names_are_rejected_before_sqlite() {
    let fixture = manual_v4(ADMISSIONS, STATES);
    let conn = wal_connection(&fixture);
    let before = readonly_snapshot(&conn);
    let uri = format!("file:{}", fixture.path.display());
    let missing = fixture
        .path
        .parent()
        .unwrap()
        .join("uri-must-not-create.sqlite");
    for path in [
        String::new(),
        ":memory:".to_owned(),
        uri.clone(),
        format!("{uri}?immutable=1"),
        format!("{uri}?mode=rw"),
        format!("file:{}?mode=rwc", missing.display()),
    ] {
        let result = JjObservationJournal::open_existing_at_path(Path::new(&path));
        assert!(
            matches!(result, Err(JournalError::Validation(_))),
            "special name reached SQLite: {path:?}"
        );
    }
    assert!(!missing.exists());
    assert_eq!(readonly_snapshot(&conn), before);
    #[cfg(unix)]
    {
        let literal = fixture.path.parent().unwrap().join("file:literal.sqlite");
        drop(JjObservationJournal::open_at_path(&literal).unwrap());
        let mut journal = JjObservationJournal::open_existing_at_path(&literal).unwrap();
        fixture.capture_first(&mut journal);
        fixture.assert_only_first(&journal);
    }
}

#[test]
fn jj_journal_existing_empty_non_sqlite_and_unrelated_databases_are_not_initialized() {
    for bytes in [b"".as_slice(), b"not a SQLite database".as_slice()] {
        let fixture = Fixture::new();
        fs::write(&fixture.path, bytes).unwrap();
        assert!(JjObservationJournal::open_existing_at_path(&fixture.path).is_err());
        // No SQLite connection survives the refused call for these closed files.
        assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    }
    let fixture = Fixture::new();
    let conn = wal_connection(&fixture);
    conn.execute_batch(
        "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES ('preserve me');",
    )
    .unwrap();
    reject_unchanged(&fixture, &conn);
}

#[test]
fn jj_journal_existing_requires_wal_without_converting_a_valid_delete_journal() {
    let fixture = manual_v4(ADMISSIONS, STATES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    shape(&conn);
    let mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
    let control = JjObservationJournal::open_read_only_at_path(&fixture.path).unwrap();
    fixture.assert_only_first(&control);
    drop(control);
    reject_unchanged(&fixture, &conn);
    drop(conn);
    let conn = wal_connection(&fixture);
    let before = readonly_snapshot(&conn);
    let journal = JjObservationJournal::open_existing_at_path(&fixture.path).unwrap();
    fixture.assert_only_first(&journal);
    assert_eq!(readonly_snapshot(&conn), before);
}

#[test]
fn jj_journal_existing_old_and_malformed_versions_in_wal_are_never_migrated() {
    for sql in [FROZEN_V1, FROZEN_V2, FROZEN_V3] {
        let fixture = Fixture::new();
        let conn = wal_connection(&fixture);
        conn.execute_batch(sql).unwrap();
        reject_unchanged(&fixture, &conn);
    }
    for value in [
        Value::Text("5".into()),
        Value::Text("04".into()),
        Value::Blob(b"4".to_vec()),
        Value::Integer(4),
        Value::Null,
    ] {
        let fixture = manual_v4(ADMISSIONS, STATES);
        let conn = wal_connection(&fixture);
        conn.execute_batch("DROP TABLE schema_metadata; CREATE TABLE schema_metadata(key, value);")
            .unwrap();
        conn.execute(
            "INSERT INTO schema_metadata VALUES ('version', ?1)",
            [value],
        )
        .unwrap();
        reject_unchanged(&fixture, &conn);
    }
    for mutation in [
        "DELETE FROM schema_metadata",
        "DROP TABLE schema_metadata",
        "ALTER TABLE schema_metadata RENAME TO ignored_metadata; CREATE TABLE schema_metadata(key, value); INSERT INTO schema_metadata VALUES ('version', '4'), ('version', '4')",
    ] {
        let fixture = manual_v4(ADMISSIONS, STATES);
        let conn = wal_connection(&fixture);
        conn.execute_batch(mutation).unwrap();
        reject_unchanged(&fixture, &conn);
    }
}

#[test]
fn jj_journal_existing_wal_requires_current_shape_across_record_families() {
    for mutation in [
        "ALTER TABLE jj_operations RENAME COLUMN payload TO wrong_payload",
        "ALTER TABLE jj_native_baselines ADD COLUMN unexpected TEXT",
        "ALTER TABLE jj_native_workspaces ADD COLUMN unexpected TEXT",
        "ALTER TABLE jj_native_admissions ADD COLUMN unexpected TEXT",
        "DROP TABLE jj_native_admission_states",
    ] {
        let fixture = manual_v4(ADMISSIONS, STATES);
        let conn = wal_connection(&fixture);
        shape(&conn);
        conn.execute_batch(mutation).unwrap();
        reject_unchanged(&fixture, &conn);
    }
}
