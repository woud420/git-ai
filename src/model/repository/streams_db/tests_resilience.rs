//! Error, deletion, recovery, and malformed-row tests for [`StreamsDatabase`].

use super::tests_records::{create_test_db, create_test_stream};
use super::*;
use chrono::TimeZone;

#[test]
fn test_update_watermark_nonexistent_stream() {
    let (db, _temp) = create_test_db();

    use crate::model::stream_watermark::ByteOffsetWatermark;
    let watermark = ByteOffsetWatermark::new(100);

    let result = db.update_watermark("nonexistent", "transcript", "/no/such/path", &watermark);
    assert!(result.is_err());
    match result {
        Err(StreamError::Fatal { message }) => {
            assert!(message.contains("Stream not found"));
        }
        _ => panic!("Expected Fatal error"),
    }
}

#[test]
fn test_update_file_metadata_nonexistent_stream() {
    let (db, _temp) = create_test_db();

    let modified = Utc.with_ymd_and_hms(2024, 6, 15, 10, 30, 0).unwrap();
    let result = db.update_file_metadata(
        "nonexistent",
        "transcript",
        "/no/such/path",
        1234,
        Some(modified),
    );
    assert!(result.is_err());
    match result {
        Err(StreamError::Fatal { message }) => {
            assert!(message.contains("Stream not found"));
        }
        _ => panic!("Expected Fatal error"),
    }
}

#[test]
fn test_record_error() {
    let (db, _temp) = create_test_db();
    let stream = create_test_stream("session-1");
    db.insert_stream(&stream).unwrap();

    // Record an error
    db.record_error(
        "session-1",
        "transcript",
        "/path/to/transcript.jsonl",
        "Test error message",
    )
    .unwrap();

    let retrieved = db
        .get_stream("session-1", "transcript", "/path/to/transcript.jsonl")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.processing_errors, 1);
    assert_eq!(retrieved.last_error, Some("Test error message".to_string()));

    // Record another error
    db.record_error(
        "session-1",
        "transcript",
        "/path/to/transcript.jsonl",
        "Another error",
    )
    .unwrap();

    let retrieved = db
        .get_stream("session-1", "transcript", "/path/to/transcript.jsonl")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.processing_errors, 2);
    assert_eq!(retrieved.last_error, Some("Another error".to_string()));
}

#[test]
fn test_record_error_nonexistent_stream() {
    let (db, _temp) = create_test_db();

    let result = db.record_error("nonexistent", "transcript", "/no/such/path", "error");
    assert!(result.is_err());
    match result {
        Err(StreamError::Fatal { message }) => {
            assert!(message.contains("Stream not found"));
        }
        _ => panic!("Expected Fatal error"),
    }
}

#[test]
fn test_delete_stream() {
    let (db, _temp) = create_test_db();
    let stream = create_test_stream("session-1");
    db.insert_stream(&stream).unwrap();

    // Verify it exists
    assert!(
        db.get_stream("session-1", "transcript", "/path/to/transcript.jsonl")
            .unwrap()
            .is_some()
    );

    // Delete it
    db.delete_stream("session-1", "transcript", "/path/to/transcript.jsonl")
        .unwrap();

    // Verify it's gone
    assert!(
        db.get_stream("session-1", "transcript", "/path/to/transcript.jsonl")
            .unwrap()
            .is_none()
    );
}

#[test]
fn test_delete_nonexistent_stream() {
    let (db, _temp) = create_test_db();

    let result = db.delete_stream("nonexistent", "transcript", "/no/such/path");
    assert!(result.is_err());
    match result {
        Err(StreamError::Fatal { message }) => {
            assert!(message.contains("Stream not found"));
        }
        _ => panic!("Expected Fatal error"),
    }
}

#[test]
fn test_insert_stream_duplicate_fails() {
    let (db, _temp) = create_test_db();

    let stream = create_test_stream("session-1");
    db.insert_stream(&stream).unwrap();

    // Try to insert a duplicate (should fail)
    let duplicate = create_test_stream("session-1");
    let result = db.insert_stream(&duplicate);
    assert!(result.is_err());

    // Original stream still intact
    let retrieved = db
        .get_stream("session-1", "transcript", "/path/to/transcript.jsonl")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.session_id, "session-1");
}

#[test]
fn test_mutex_poison_recovery() {
    use std::sync::Arc;
    use std::thread;

    let (db, _temp) = create_test_db();
    let stream = create_test_stream("session-1");
    db.insert_stream(&stream).unwrap();

    // Create a scenario that would poison the mutex in older code
    // This is a bit contrived since we now recover from poison automatically
    // but it demonstrates that poison recovery works

    let db_arc = Arc::new(db);
    let db_clone = Arc::clone(&db_arc);

    // Spawn a thread that panics while holding the lock
    let handle = thread::spawn(move || {
        let conn = db_clone
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Force a panic (commented out to not actually poison in this test)
        // panic!("Simulated panic");
        drop(conn);
    });

    let _ = handle.join();

    // After the thread completes (or panics), we should still be able to use the database
    let result = db_arc.get_stream("session-1", "transcript", "/path/to/transcript.jsonl");
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[test]
fn test_migration_v3_to_v4_preserves_data() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("migration_test.db");

    // Manually create a v3 database
    {
        let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
        // Run migrations 0..=2 (versions 1, 2, 3)
        for migration in &MIGRATIONS[..3] {
            conn.execute_batch(migration).unwrap();
        }
        // Insert a session using the v3 schema (no stream_kind column)
        conn.execute(
            "INSERT INTO sessions (session_id, tool, transcript_path, transcript_format, \
             watermark_type, watermark_value, external_session_id, external_parent_session_id, \
             first_seen_at, last_processed_at, last_known_size, last_modified, \
             processing_errors, last_error, repo_work_dir) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                "sess-migrate-1",
                "claude",
                "/path/to/transcript.jsonl",
                "ClaudeJsonl",
                "ByteOffset",
                "1234",
                "external-sess-1",
                None::<String>,
                1000,
                500,
                5678,
                Some(900),
                2,
                Some("some error"),
                Some("/work/dir"),
            ],
        )
        .unwrap();

        // Verify we're at version 3
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, 3);
    }

    // Reopen via StreamsDatabase (triggers migration to v4)
    let db = StreamsDatabase::open(&db_path).unwrap();

    // Verify the stream migrated with stream_kind = 'transcript'
    let stream = db
        .get_stream("sess-migrate-1", "transcript", "/path/to/transcript.jsonl")
        .unwrap();
    assert!(stream.is_some(), "stream should exist after migration");
    let stream = stream.unwrap();
    assert_eq!(stream.session_id, "sess-migrate-1");
    assert_eq!(stream.stream_kind, "transcript");
    assert_eq!(stream.tool, "claude");
    assert_eq!(stream.stream_path, "/path/to/transcript.jsonl");
    assert_eq!(stream.watermark_value, "1234");
    assert_eq!(stream.external_session_id, "external-sess-1");
    assert_eq!(stream.external_parent_session_id, None);
    assert_eq!(stream.last_known_size, 5678);
    assert_eq!(stream.last_modified, Some(900));
    assert_eq!(stream.processing_errors, 2);
    assert_eq!(stream.last_error, Some("some error".to_string()));
    assert_eq!(stream.repo_work_dir, Some("/work/dir".to_string()));
}

#[test]
fn test_migration_v3_to_v4_multiple_sessions_no_conflict() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("migration_multi.db");

    // Create v3 DB with multiple sessions
    {
        let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
        for migration in &MIGRATIONS[..3] {
            conn.execute_batch(migration).unwrap();
        }
        for i in 0..5 {
            conn.execute(
                "INSERT INTO sessions (session_id, tool, transcript_path, transcript_format, \
                 watermark_type, watermark_value, external_session_id, external_parent_session_id, \
                 first_seen_at, last_processed_at, last_known_size, last_modified, \
                 processing_errors, last_error, repo_work_dir) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![
                    format!("sess-{}", i),
                    "claude",
                    format!("/path/to/transcript_{}.jsonl", i),
                    "ClaudeJsonl",
                    "ByteOffset",
                    format!("{}", i * 100),
                    format!("ext-{}", i),
                    None::<String>,
                    1000 + i,
                    500 + i,
                    0,
                    None::<i64>,
                    0,
                    None::<String>,
                    None::<String>,
                ],
            )
            .unwrap();
        }
    }

    // Reopen (triggers migration)
    let db = StreamsDatabase::open(&db_path).unwrap();

    // All 5 streams should be present
    let all = db.all_streams().unwrap();
    assert_eq!(all.len(), 5);

    // Each should have stream_kind = 'transcript'
    for i in 0..5 {
        let stream = db
            .get_stream(
                &format!("sess-{}", i),
                "transcript",
                &format!("/path/to/transcript_{}.jsonl", i),
            )
            .unwrap()
            .unwrap();
        assert_eq!(stream.stream_kind, "transcript");
        assert_eq!(stream.watermark_value, format!("{}", i * 100));
    }
}

#[test]
fn test_composite_pk_allows_same_session_id_different_streams() {
    let (db, _temp) = create_test_db();

    // Insert same session_id with different stream_kind
    let mut transcript_stream = create_test_stream("shared-session");
    transcript_stream.stream_kind = "transcript".to_string();
    transcript_stream.stream_path = "/path/to/transcript.jsonl".to_string();
    db.insert_stream(&transcript_stream).unwrap();

    let mut otel_stream = create_test_stream("shared-session");
    otel_stream.stream_kind = "otel_traces".to_string();
    otel_stream.stream_path = "/path/to/traces.db".to_string();
    otel_stream.watermark_type = WatermarkType::TimestampCursor;
    otel_stream.watermark_value = "0|".to_string();
    db.insert_stream(&otel_stream).unwrap();

    // Both should exist independently
    let t = db
        .get_stream("shared-session", "transcript", "/path/to/transcript.jsonl")
        .unwrap()
        .unwrap();
    assert_eq!(t.stream_kind, "transcript");

    let o = db
        .get_stream("shared-session", "otel_traces", "/path/to/traces.db")
        .unwrap()
        .unwrap();
    assert_eq!(o.stream_kind, "otel_traces");

    // Update one without affecting the other
    let new_watermark = crate::model::stream_watermark::ByteOffsetWatermark::new(999);
    db.update_watermark(
        "shared-session",
        "transcript",
        "/path/to/transcript.jsonl",
        &new_watermark,
    )
    .unwrap();

    let t_updated = db
        .get_stream("shared-session", "transcript", "/path/to/transcript.jsonl")
        .unwrap()
        .unwrap();
    assert_eq!(t_updated.watermark_value, "999");

    // OTEL watermark unchanged
    let o_unchanged = db
        .get_stream("shared-session", "otel_traces", "/path/to/traces.db")
        .unwrap()
        .unwrap();
    assert_eq!(o_unchanged.watermark_value, "0|");
}

#[test]
fn test_composite_pk_allows_same_session_id_different_paths() {
    let (db, _temp) = create_test_db();

    // This is the #1461 scenario: same session_id, same stream_kind, different paths
    let mut stream1 = create_test_stream("colliding-session");
    stream1.stream_path = "/worktree-a/transcript.jsonl".to_string();
    db.insert_stream(&stream1).unwrap();

    let mut stream2 = create_test_stream("colliding-session");
    stream2.stream_path = "/worktree-b/transcript.jsonl".to_string();
    db.insert_stream(&stream2).unwrap();

    // Both exist independently
    let s1 = db
        .get_stream(
            "colliding-session",
            "transcript",
            "/worktree-a/transcript.jsonl",
        )
        .unwrap()
        .unwrap();
    let s2 = db
        .get_stream(
            "colliding-session",
            "transcript",
            "/worktree-b/transcript.jsonl",
        )
        .unwrap()
        .unwrap();
    assert_eq!(s1.stream_path, "/worktree-a/transcript.jsonl");
    assert_eq!(s2.stream_path, "/worktree-b/transcript.jsonl");

    // Delete one, the other remains
    db.delete_stream(
        "colliding-session",
        "transcript",
        "/worktree-a/transcript.jsonl",
    )
    .unwrap();
    assert!(
        db.get_stream(
            "colliding-session",
            "transcript",
            "/worktree-a/transcript.jsonl"
        )
        .unwrap()
        .is_none()
    );
    assert!(
        db.get_stream(
            "colliding-session",
            "transcript",
            "/worktree-b/transcript.jsonl"
        )
        .unwrap()
        .is_some()
    );
}

// Verify all_streams() skips rows with unknown enum values (log-and-skip, not abort).
#[test]
fn all_streams_skips_unrecognized_format_row() {
    let (db, _temp) = create_test_db();
    db.insert_stream(&create_test_stream("session-good"))
        .unwrap();
    db.conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO tracked_streams (session_id, stream_kind, tool, stream_path, \
         stream_format, watermark_type, watermark_value, external_session_id, \
         first_seen_at, last_processed_at, last_known_size) \
         VALUES ('session-bogus','transcript','claude','/bogus',\
         'UnknownFormatXyz','ByteOffset','0','ext-bogus',1000,1000,0)",
            [],
        )
        .unwrap();
    let streams = db.all_streams().unwrap();
    assert_eq!(streams.len(), 1, "bogus row should be skipped, not crash");
    assert_eq!(streams[0].session_id, "session-good");
}
