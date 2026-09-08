use super::*;
use crate::model::repository::sqlite::assert_persisted_schema_version;
use tempfile::TempDir;

fn create_test_db() -> (InternalDatabase, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();

    let mut db = InternalDatabase {
        conn,
        _db_path: db_path.clone(),
    };
    db.initialize_schema().unwrap();

    (db, temp_dir)
}

#[test]
fn test_initialize_schema() {
    let (db, _temp_dir) = create_test_db();

    // Verify tables exist
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='prompts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    // Verify schema_metadata exists
    assert_persisted_schema_version(&db.conn, "3");
}

#[test]
fn test_initialize_schema_handles_preexisting_cas_cache_table() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("concurrent-init.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();

    // Simulate a partial migration state from a concurrent process:
    // schema version indicates cas_cache is missing, but the table already exists.
    conn.execute_batch(
        r#"
            CREATE TABLE schema_metadata (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );
            INSERT INTO schema_metadata (key, value) VALUES ('version', '2');
            CREATE TABLE cas_cache (
                hash TEXT PRIMARY KEY NOT NULL,
                messages TEXT NOT NULL,
                cached_at INTEGER NOT NULL
            );
            "#,
    )
    .unwrap();

    let mut db = InternalDatabase {
        conn,
        _db_path: db_path,
    };
    db.initialize_schema().unwrap();

    assert_persisted_schema_version(&db.conn, "3");
}

#[test]
fn test_database_path() {
    let override_path = std::env::var("GIT_AI_TEST_DB_PATH").ok();
    let path = InternalDatabase::database_path().unwrap();
    if let Some(override_path) = override_path {
        assert_eq!(path, PathBuf::from(override_path));
    } else {
        assert!(path.to_string_lossy().contains(".git-ai"));
        assert!(path.to_string_lossy().contains("internal"));
        assert!(path.to_string_lossy().ends_with("db"));
    }
}

// CAS sync queue tests

#[test]
fn test_cas_sync_queue_schema() {
    let (db, _temp_dir) = create_test_db();

    // Verify cas_sync_queue table exists
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cas_sync_queue'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    // Verify status column has correct default and check constraint
    let status: String = db
        .conn
        .query_row(
            "SELECT dflt_value FROM pragma_table_info('cas_sync_queue') WHERE name='status'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "'pending'");
}

#[test]
fn test_enqueue_cas_object_with_metadata() {
    let (mut db, _temp_dir) = create_test_db();

    let mut metadata = HashMap::new();
    metadata.insert("key1".to_string(), "value1".to_string());
    metadata.insert("key2".to_string(), "value2".to_string());

    let json_data = serde_json::json!({"test": "data", "number": 123});

    // Enqueue an object with metadata
    let hash = db.enqueue_cas_object(&json_data, Some(&metadata)).unwrap();

    // Verify metadata was stored correctly
    let metadata_json: String = db
        .conn
        .query_row(
            "SELECT metadata FROM cas_sync_queue WHERE hash = ?",
            params![&hash],
            |row| row.get(0),
        )
        .unwrap();

    let stored_metadata: HashMap<String, String> = serde_json::from_str(&metadata_json).unwrap();
    assert_eq!(stored_metadata.get("key1"), Some(&"value1".to_string()));
    assert_eq!(stored_metadata.get("key2"), Some(&"value2".to_string()));

    // Verify dequeue returns metadata correctly
    let batch = db.dequeue_cas_batch(10).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].hash, hash);
    // Data is canonicalized JSON
    let stored_json: serde_json::Value = serde_json::from_str(&batch[0].data).unwrap();
    assert_eq!(stored_json, json_data);
    assert_eq!(batch[0].metadata.get("key1"), Some(&"value1".to_string()));
    assert_eq!(batch[0].metadata.get("key2"), Some(&"value2".to_string()));
}

#[test]
fn test_enqueue_cas_object() {
    let (mut db, _temp_dir) = create_test_db();

    let json_data = serde_json::json!({"key": "value"});

    // Enqueue an object
    let hash = db.enqueue_cas_object(&json_data, None).unwrap();

    // Verify it was inserted with correct defaults
    let (stored_hash, stored_data, metadata, status, attempts): (
        String,
        String,
        String,
        String,
        u32,
    ) = db
        .conn
        .query_row(
            "SELECT hash, data, metadata, status, attempts FROM cas_sync_queue WHERE hash = ?",
            params![&hash],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(stored_hash, hash);
    // Data should be canonicalized JSON
    let stored_json: serde_json::Value = serde_json::from_str(&stored_data).unwrap();
    assert_eq!(stored_json, json_data);
    assert_eq!(status, "pending");
    assert_eq!(attempts, 0);
    assert_eq!(metadata, "{}");
}

#[test]
fn test_enqueue_duplicate_hash() {
    let (mut db, _temp_dir) = create_test_db();

    // Same JSON content should produce same hash
    let json_data = serde_json::json!({"same": "content"});

    // Enqueue the same content twice
    let hash1 = db.enqueue_cas_object(&json_data, None).unwrap();
    let hash2 = db.enqueue_cas_object(&json_data, None).unwrap();

    // Both calls should return the same hash
    assert_eq!(hash1, hash2);

    // Verify only one record exists (INSERT OR IGNORE)
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM cas_sync_queue WHERE hash = ?",
            params![&hash1],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_dequeue_cas_batch() {
    let (mut db, _temp_dir) = create_test_db();

    // Enqueue multiple objects with different content
    db.enqueue_cas_object(&serde_json::json!({"id": 1}), None)
        .unwrap();
    db.enqueue_cas_object(&serde_json::json!({"id": 2}), None)
        .unwrap();
    db.enqueue_cas_object(&serde_json::json!({"id": 3}), None)
        .unwrap();

    // Dequeue batch of 2
    let batch = db.dequeue_cas_batch(2).unwrap();
    assert_eq!(batch.len(), 2);

    // Verify records are marked as processing
    let processing_count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM cas_sync_queue WHERE status = 'processing'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(processing_count, 2);

    // Verify one is still pending
    let pending_count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM cas_sync_queue WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending_count, 1);
}

#[test]
fn test_dequeue_respects_next_retry() {
    let (mut db, _temp_dir) = create_test_db();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let hash1 = "hash1";
    let hash2 = "hash2";
    let data1 = "data1";
    let data2 = "data2";

    // Insert one record ready to retry (past)
    db.conn.execute(
            "INSERT INTO cas_sync_queue (hash, data, metadata, status, attempts, next_retry_at, created_at) VALUES (?, ?, '{}', 'pending', 0, ?, ?)",
            params![hash1, data1, now - 100, now],
        ).unwrap();

    // Insert one record not ready yet (future)
    db.conn.execute(
            "INSERT INTO cas_sync_queue (hash, data, metadata, status, attempts, next_retry_at, created_at) VALUES (?, ?, '{}', 'pending', 0, ?, ?)",
            params![hash2, data2, now + 1000, now],
        ).unwrap();

    // Dequeue should only return the first one
    let batch = db.dequeue_cas_batch(10).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].hash, hash1);
}

#[test]
fn test_dequeue_locks_records() {
    let (mut db, _temp_dir) = create_test_db();

    let json_data = serde_json::json!({"test": "lock"});
    let hash = db.enqueue_cas_object(&json_data, None).unwrap();

    // Dequeue
    let batch = db.dequeue_cas_batch(10).unwrap();
    assert_eq!(batch.len(), 1);

    // Verify status is 'processing'
    let status: String = db
        .conn
        .query_row(
            "SELECT status FROM cas_sync_queue WHERE hash = ?",
            params![&hash],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "processing");

    // Verify processing_started_at is set
    let processing_started_at: Option<i64> = db
        .conn
        .query_row(
            "SELECT processing_started_at FROM cas_sync_queue WHERE hash = ?",
            params![&hash],
            |row| row.get(0),
        )
        .unwrap();
    assert!(processing_started_at.is_some());

    // Try to dequeue again - should get empty (already locked)
    let batch2 = db.dequeue_cas_batch(10).unwrap();
    assert_eq!(batch2.len(), 0);
}

#[test]
fn test_stale_lock_recovery() {
    let (mut db, _temp_dir) = create_test_db();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let hash = "hash1";
    let data = "data1";

    // Insert a record in 'processing' state with old timestamp (>10 minutes ago)
    let stale_time = now - 700; // 11+ minutes ago
    db.conn.execute(
            "INSERT INTO cas_sync_queue (hash, data, metadata, status, attempts, next_retry_at, processing_started_at, created_at) VALUES (?, ?, '{}', 'processing', 0, ?, ?, ?)",
            params![hash, data, now, stale_time, now],
        ).unwrap();

    // Dequeue should recover the stale lock
    let batch = db.dequeue_cas_batch(10).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].hash, hash);
}

#[test]
fn test_max_attempts_limit() {
    let (mut db, _temp_dir) = create_test_db();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let hash1 = "hash1";
    let hash2 = "hash2";
    let data1 = "data1";
    let data2 = "data2";

    // Insert a record with 6 attempts (max reached)
    db.conn.execute(
            "INSERT INTO cas_sync_queue (hash, data, metadata, status, attempts, next_retry_at, created_at) VALUES (?, ?, '{}', 'pending', 6, ?, ?)",
            params![hash1, data1, now - 100, now],
        ).unwrap();

    // Insert a record with 5 attempts (still eligible)
    db.conn.execute(
            "INSERT INTO cas_sync_queue (hash, data, metadata, status, attempts, next_retry_at, created_at) VALUES (?, ?, '{}', 'pending', 5, ?, ?)",
            params![hash2, data2, now - 100, now],
        ).unwrap();

    // Dequeue should only return the one with 5 attempts
    let batch = db.dequeue_cas_batch(10).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].hash, hash2);
    assert_eq!(batch[0].attempts, 5);
}

#[test]
fn test_update_cas_sync_failure() {
    let (mut db, _temp_dir) = create_test_db();

    db.enqueue_cas_object(&serde_json::json!({"test": "failure"}), None)
        .unwrap();
    let batch = db.dequeue_cas_batch(10).unwrap();
    let record = &batch[0];

    // Update with failure
    db.update_cas_sync_failure(record.id, "test error").unwrap();

    // Verify status is back to 'pending'
    let status: String = db
        .conn
        .query_row(
            "SELECT status FROM cas_sync_queue WHERE id = ?",
            params![record.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "pending");

    // Verify processing_started_at is cleared
    let processing_started_at: Option<i64> = db
        .conn
        .query_row(
            "SELECT processing_started_at FROM cas_sync_queue WHERE id = ?",
            params![record.id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(processing_started_at.is_none());

    // Verify attempts incremented
    let attempts: u32 = db
        .conn
        .query_row(
            "SELECT attempts FROM cas_sync_queue WHERE id = ?",
            params![record.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 1);

    // Verify error recorded
    let error: String = db
        .conn
        .query_row(
            "SELECT last_sync_error FROM cas_sync_queue WHERE id = ?",
            params![record.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(error, "test error");
}

#[test]
fn test_delete_cas_sync_record() {
    let (mut db, _temp_dir) = create_test_db();

    db.enqueue_cas_object(&serde_json::json!({"test": "delete"}), None)
        .unwrap();
    let batch = db.dequeue_cas_batch(10).unwrap();
    let record = &batch[0];

    // Delete the record
    db.delete_cas_sync_record(record.id).unwrap();

    // Verify it's gone
    let count: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM cas_sync_queue WHERE id = ?",
            params![record.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

// CAS cache tests

#[test]
fn test_cas_cache_get_miss() {
    let (db, _temp_dir) = create_test_db();
    let result = db.get_cas_cache("nonexistent_hash").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_cas_cache_set_and_get() {
    let (mut db, _temp_dir) = create_test_db();
    let hash = "abc123def456";
    let messages = r#"[{"type":"user","text":"hello"}]"#;

    db.set_cas_cache(hash, messages).unwrap();

    let result = db.get_cas_cache(hash).unwrap();
    assert_eq!(result, Some(messages.to_string()));
}

#[test]
fn test_cas_cache_overwrite() {
    let (mut db, _temp_dir) = create_test_db();
    let hash = "abc123def456";
    let messages1 = r#"[{"type":"user","text":"v1"}]"#;
    let messages2 = r#"[{"type":"user","text":"v2"}]"#;

    db.set_cas_cache(hash, messages1).unwrap();
    db.set_cas_cache(hash, messages2).unwrap();

    let result = db.get_cas_cache(hash).unwrap();
    assert_eq!(result, Some(messages2.to_string()));
}

#[test]
fn test_exponential_backoff() {
    let now = 1000000i64;

    // Test each attempt's backoff
    assert_eq!(calculate_next_retry(1, now), now + 5 * 60); // 5 min
    assert_eq!(calculate_next_retry(2, now), now + 30 * 60); // 30 min
    assert_eq!(calculate_next_retry(3, now), now + 2 * 60 * 60); // 2 hours
    assert_eq!(calculate_next_retry(4, now), now + 6 * 60 * 60); // 6 hours
    assert_eq!(calculate_next_retry(5, now), now + 12 * 60 * 60); // 12 hours
    assert_eq!(calculate_next_retry(6, now), now + 24 * 60 * 60); // 24 hours
    assert_eq!(calculate_next_retry(7, now), now + 24 * 60 * 60); // 24 hours (max)
}
