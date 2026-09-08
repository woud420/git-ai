use super::*;
use crate::model::repository::sqlite::assert_persisted_schema_version;
use tempfile::TempDir;

/// Open a fresh in-memory database (via a temp file) without using the global singleton.
fn create_test_db() -> (NotesDatabase, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test-notes.db");

    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    conn.execute_batch(
        r#"
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            "#,
    )
    .unwrap();

    let mut db = NotesDatabase { conn };
    db.initialize_schema().unwrap();

    (db, temp_dir)
}

// --- Schema tests ---

#[test]
fn test_fresh_db_creates_notes_table() {
    let (db, _tmp) = create_test_db();

    let table_count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='notes'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 1, "notes table should exist after init");

    assert_persisted_schema_version(&db.conn, "2");
}

#[test]
fn test_notes_table_has_expected_columns() {
    let (db, _tmp) = create_test_db();

    // PRAGMA table_info returns one row per column
    let mut stmt = db.conn.prepare("PRAGMA table_info(notes)").unwrap();
    let column_names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let required = [
        "commit_sha",
        "content",
        "synced",
        "attempts",
        "last_sync_error",
        "last_sync_at",
        "next_retry_at",
        "processing_started_at",
        "created_at",
        "updated_at",
    ];
    for col in &required {
        assert!(
            column_names.iter().any(|c| c == col),
            "column '{}' is missing; found: {:?}",
            col,
            column_names
        );
    }
}

// --- Upsert / round-trip ---

#[test]
fn test_upsert_and_get_note_roundtrip() {
    let (mut db, _tmp) = create_test_db();

    db.upsert_note("abc123", "content1").unwrap();
    let retrieved = db.get_note("abc123").unwrap();
    assert_eq!(retrieved, Some("content1".to_string()));
}

#[test]
fn test_upsert_missing_sha_returns_none() {
    let (db, _tmp) = create_test_db();
    assert_eq!(db.get_note("nonexistent").unwrap(), None);
}

#[test]
fn test_upsert_new_content_resets_synced() {
    let (mut db, _tmp) = create_test_db();

    db.upsert_note("sha1", "original").unwrap();
    // Mark as synced manually
    db.conn
        .execute("UPDATE notes SET synced = 1 WHERE commit_sha = 'sha1'", [])
        .unwrap();

    // Upsert with different content → should reset synced
    db.upsert_note("sha1", "updated").unwrap();
    let synced: i64 = db
        .conn
        .query_row(
            "SELECT synced FROM notes WHERE commit_sha = 'sha1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(synced, 0, "synced should be reset when content changes");
}

#[test]
fn test_upsert_same_content_preserves_synced() {
    let (mut db, _tmp) = create_test_db();

    db.upsert_note("sha1", "same content").unwrap();
    db.conn
        .execute("UPDATE notes SET synced = 1 WHERE commit_sha = 'sha1'", [])
        .unwrap();

    // Upsert with identical content → synced should stay 1
    db.upsert_note("sha1", "same content").unwrap();
    let synced: i64 = db
        .conn
        .query_row(
            "SELECT synced FROM notes WHERE commit_sha = 'sha1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        synced, 1,
        "synced should be preserved when content is unchanged"
    );
}

// --- Dequeue / mark_synced round-trip ---

#[test]
fn test_dequeue_returns_pending_notes() {
    let (mut db, _tmp) = create_test_db();

    db.upsert_note("sha_a", "content_a").unwrap();
    db.upsert_note("sha_b", "content_b").unwrap();

    let batch = db.dequeue_pending(10).unwrap();
    assert_eq!(batch.len(), 2);
}

#[test]
fn test_dequeue_mark_synced_roundtrip() {
    let (mut db, _tmp) = create_test_db();

    db.upsert_note("sha1", "data").unwrap();

    // First dequeue should return the row.
    let batch = db.dequeue_pending(10).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].commit_sha, "sha1");

    // Mark synced
    let shas: Vec<String> = batch.iter().map(|p| p.commit_sha.clone()).collect();
    let updated = db.mark_synced(&shas).unwrap();
    assert_eq!(updated, 1);

    // Second dequeue should return nothing (row is now synced = 1).
    let batch2 = db.dequeue_pending(10).unwrap();
    assert!(batch2.is_empty(), "no pending rows after mark_synced");
}

#[test]
fn test_dequeue_does_not_return_synced_rows() {
    let (mut db, _tmp) = create_test_db();

    db.cache_synced_notes(&[("sha_synced".to_string(), "cached".to_string())])
        .unwrap();

    let batch = db.dequeue_pending(10).unwrap();
    assert!(
        batch.is_empty(),
        "cache_synced_notes rows must not appear in dequeue_pending"
    );
}

#[test]
fn test_count_pending_uploadable_excludes_deferred_and_processing_rows() {
    let (mut db, _tmp) = create_test_db();

    for sha in ["ready", "backoff", "processing", "permanent"] {
        db.upsert_note(sha, "content").unwrap();
    }
    db.conn
        .execute(
            "UPDATE notes SET attempts = 1, next_retry_at = ?1 WHERE commit_sha = 'backoff'",
            params![unix_now() + 3_600],
        )
        .unwrap();
    db.conn
        .execute(
            "UPDATE notes SET processing_started_at = ?1 WHERE commit_sha = 'processing'",
            params![unix_now()],
        )
        .unwrap();
    db.conn
        .execute(
            "UPDATE notes SET attempts = 6 WHERE commit_sha = 'permanent'",
            [],
        )
        .unwrap();

    assert_eq!(db.count_pending_uploadable().unwrap(), 1);
}

// --- cache_synced_notes ---

#[test]
fn test_cache_synced_notes_inserts_with_synced_1() {
    let (mut db, _tmp) = create_test_db();

    db.cache_synced_notes(&[
        ("commit1".to_string(), "note1".to_string()),
        ("commit2".to_string(), "note2".to_string()),
    ])
    .unwrap();

    // Verify both rows exist and are synced = 1
    let synced: i64 = db
        .conn
        .query_row(
            "SELECT synced FROM notes WHERE commit_sha = 'commit1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(synced, 1);

    let synced2: i64 = db
        .conn
        .query_row(
            "SELECT synced FROM notes WHERE commit_sha = 'commit2'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(synced2, 1);
}

// --- mark_failed ---

#[test]
fn test_mark_failed_increments_attempts_and_schedules_retry() {
    let (mut db, _tmp) = create_test_db();

    db.upsert_note("sha_fail", "data").unwrap();

    // Dequeue so processing_started_at is set
    let _ = db.dequeue_pending(10).unwrap();

    let before_time = unix_now();
    db.mark_failed(&["sha_fail".to_string()], "connection timeout")
        .unwrap();

    let (attempts, next_retry_at, error): (i64, i64, String) = db
            .conn
            .query_row(
                "SELECT attempts, next_retry_at, last_sync_error FROM notes WHERE commit_sha = 'sha_fail'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

    assert_eq!(attempts, 1, "attempts should be incremented");
    assert!(
        next_retry_at > before_time,
        "next_retry_at should be in the future"
    );
    assert_eq!(error, "connection timeout");
}

#[test]
fn test_mark_failed_processing_started_cleared() {
    let (mut db, _tmp) = create_test_db();

    db.upsert_note("sha_lock", "data").unwrap();
    let _ = db.dequeue_pending(10).unwrap(); // sets processing_started_at

    db.mark_failed(&["sha_lock".to_string()], "err").unwrap();

    let processing_started_at: Option<i64> = db
        .conn
        .query_row(
            "SELECT processing_started_at FROM notes WHERE commit_sha = 'sha_lock'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        processing_started_at.is_none(),
        "processing_started_at should be cleared after mark_failed"
    );
}

// --- get_notes (batch) ---

#[test]
fn test_get_notes_batch() {
    let (mut db, _tmp) = create_test_db();

    db.upsert_note("sha1", "c1").unwrap();
    db.upsert_note("sha2", "c2").unwrap();

    let results = db.get_notes(&["sha1", "sha2", "sha_missing"]).unwrap();
    assert_eq!(results.get("sha1"), Some(&"c1".to_string()));
    assert_eq!(results.get("sha2"), Some(&"c2".to_string()));
    assert!(
        !results.contains_key("sha_missing"),
        "missing SHA should not be in result"
    );
}

// --- database_path ---

#[test]
#[serial_test::serial(notes_db_env)]
fn test_database_path_contains_expected_segments() {
    // Without the test env var set we expect the path to include .git-ai/internal/notes-db
    // (this test verifies the non-override branch at the schema level; in CI the HOME is
    // always set so dirs::home_dir() returns Some).
    unsafe {
        std::env::remove_var("GIT_AI_TEST_NOTES_DB_PATH");
    }
    let path = NotesDatabase::database_path().unwrap();
    let path_str = path.to_string_lossy();
    assert!(
        path_str.contains(".git-ai"),
        "path should contain .git-ai: {}",
        path_str
    );
    assert!(
        path_str.contains("internal"),
        "path should contain internal: {}",
        path_str
    );
    assert!(
        path_str.ends_with("notes-db"),
        "path should end with notes-db: {}",
        path_str
    );
}

#[test]
fn test_local_notes_are_not_dequeued_or_evicted() {
    let (mut db, _tmp) = create_test_db();

    db.upsert_local_note("a".repeat(40).as_str(), "local-note")
        .unwrap();
    db.upsert_note("b".repeat(40).as_str(), "queued-note")
        .unwrap();

    // Only the queue-origin row is dequeued for upload.
    let pending = db.dequeue_pending(10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].commit_sha, "b".repeat(40));

    // Eviction never touches local-primary rows even at zero thresholds.
    let evicted = db.evict_stale_cache(0, -1).unwrap();
    assert_eq!(evicted, 0);
    assert_eq!(
        db.get_note("a".repeat(40).as_str()).unwrap().as_deref(),
        Some("local-note")
    );
}

#[test]
fn test_cache_import_does_not_clobber_local_note() {
    let (mut db, _tmp) = create_test_db();
    let sha = "c".repeat(40);

    db.upsert_local_note(&sha, "local-truth").unwrap();
    db.cache_synced_notes(&[(sha.clone(), "stale-remote-copy".to_string())])
        .unwrap();

    assert_eq!(
        db.get_note(&sha).unwrap().as_deref(),
        Some("local-truth"),
        "cache imports must not overwrite local-primary rows"
    );
}

#[test]
fn test_get_local_notes_returns_only_local_rows() {
    let (mut db, _tmp) = create_test_db();
    db.upsert_local_note("d".repeat(40).as_str(), "local")
        .unwrap();
    db.upsert_note("e".repeat(40).as_str(), "queued").unwrap();
    db.cache_synced_notes(&[("f".repeat(40), "cached".to_string())])
        .unwrap();

    let local = db.get_local_notes().unwrap();
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].0, "d".repeat(40));
}

#[test]
fn test_migration_v1_to_v2_marks_existing_rows_as_queue() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("notes-db");

    // Create a v1 database by hand.
    {
        let conn = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
        conn.execute_batch(
            r#"
                CREATE TABLE schema_metadata (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
                INSERT INTO schema_metadata (key, value) VALUES ('version', '1');
                CREATE TABLE notes (
                    commit_sha              TEXT PRIMARY KEY NOT NULL,
                    content                 TEXT NOT NULL,
                    synced                  INTEGER NOT NULL DEFAULT 0,
                    attempts                INTEGER NOT NULL DEFAULT 0,
                    last_sync_error         TEXT,
                    last_sync_at            INTEGER,
                    next_retry_at           INTEGER NOT NULL DEFAULT 0,
                    processing_started_at   INTEGER,
                    created_at              INTEGER NOT NULL,
                    updated_at              INTEGER NOT NULL
                );
                INSERT INTO notes (commit_sha, content, synced, created_at, updated_at)
                VALUES ('legacy-sha', 'legacy-content', 0, 1, 1);
                "#,
        )
        .unwrap();
    }

    let db = NotesDatabase::open_at_path(&path).unwrap();
    let origin: String = db
        .conn
        .query_row(
            "SELECT origin FROM notes WHERE commit_sha = 'legacy-sha'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(origin, "queue", "legacy rows belong to the HTTP queue");
}
