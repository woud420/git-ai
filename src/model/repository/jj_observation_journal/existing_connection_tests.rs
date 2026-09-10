use super::JjObservationJournal;
use crate::model::repository::sqlite::MEMORY_LIMIT_CACHE_SIZE_KIB;

#[test]
fn jj_journal_existing_connection_is_writable_with_full_durability_and_bounded_settings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("journal.sqlite");
    drop(JjObservationJournal::open_at_path(&path).unwrap());
    let journal = JjObservationJournal::open_existing_at_path(&path).unwrap();
    assert!(
        !journal
            .conn
            .is_readonly(rusqlite::DatabaseName::Main)
            .unwrap()
    );
    for (pragma, expected) in [
        ("cache_size", i64::from(MEMORY_LIMIT_CACHE_SIZE_KIB)),
        ("foreign_keys", 1),
        ("busy_timeout", 250),
        ("synchronous", 2),
        ("temp_store", 2),
    ] {
        let actual: i64 = journal
            .conn
            .pragma_query_value(None, pragma, |row| row.get(0))
            .unwrap();
        assert_eq!(actual, expected, "unexpected {pragma}");
    }
    let mode: String = journal
        .conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    assert!(journal.conn.is_autocommit());
}
