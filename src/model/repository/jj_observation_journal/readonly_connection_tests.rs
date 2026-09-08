use super::JjObservationJournal;
use crate::model::repository::sqlite::MEMORY_LIMIT_CACHE_SIZE_KIB;

#[test]
fn jj_journal_readonly_connection_enforces_main_readonly_and_bounded_settings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("journal.sqlite");
    drop(JjObservationJournal::open_at_path(&path).unwrap());
    let journal = JjObservationJournal::open_read_only_at_path(&path).unwrap();
    assert!(
        journal
            .conn
            .is_readonly(rusqlite::DatabaseName::Main)
            .unwrap()
    );
    let cache: i64 = journal
        .conn
        .pragma_query_value(None, "cache_size", |row| row.get(0))
        .unwrap();
    assert_eq!(cache, i64::from(MEMORY_LIMIT_CACHE_SIZE_KIB));
    let foreign_keys: bool = journal
        .conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .unwrap();
    assert!(foreign_keys);
    let timeout: u64 = journal
        .conn
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .unwrap();
    assert_eq!(timeout, 250);
    let mode: String = journal
        .conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let error = journal
        .conn
        .execute("UPDATE schema_metadata SET value='4'", [])
        .unwrap_err();
    assert!(
        matches!(error, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ffi::ErrorCode::ReadOnly)
    );
}
