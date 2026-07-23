//! Schema, CRUD, and watermark tests for [`StreamsDatabase`].

use super::*;
use crate::model::stream_types::StreamFormat;
use chrono::TimeZone;
use tempfile::NamedTempFile;

pub(super) fn create_test_db() -> (StreamsDatabase, NamedTempFile) {
    let temp_file = NamedTempFile::new().unwrap();
    let db = StreamsDatabase::open(temp_file.path()).unwrap();
    (db, temp_file)
}

pub(super) fn create_test_stream(session_id: &str) -> StreamRecord {
    StreamRecord {
        session_id: session_id.to_string(),
        stream_kind: "transcript".to_string(),
        tool: "claude".to_string(),
        stream_path: "/path/to/transcript.jsonl".to_string(),
        stream_format: StreamFormat::ClaudeJsonl,
        watermark_type: WatermarkType::ByteOffset,
        watermark_value: "0".to_string(),
        external_session_id: "thread-123".to_string(),
        external_parent_session_id: None,
        first_seen_at: 1704067200,
        last_processed_at: 1704067200,
        last_known_size: 0,
        last_modified: Some(1704067200),
        processing_errors: 0,
        last_error: None,
        repo_work_dir: None,
    }
}

#[test]
fn test_database_open_creates_schema() {
    let temp_file = NamedTempFile::new().unwrap();
    let db = StreamsDatabase::open(temp_file.path()).unwrap();

    // Verify schema exists
    let conn = db.conn.lock().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='tracked_streams'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_database_wal_mode_enabled() {
    let temp_file = NamedTempFile::new().unwrap();
    let db = StreamsDatabase::open(temp_file.path()).unwrap();

    let conn = db.conn.lock().unwrap();
    let mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
}

#[test]
fn test_insert_and_get_stream() {
    let (db, _temp) = create_test_db();
    let stream = create_test_stream("session-1");

    db.insert_stream(&stream).unwrap();

    let retrieved = db
        .get_stream("session-1", "transcript", "/path/to/transcript.jsonl")
        .unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap(), stream);
}

#[test]
fn test_get_nonexistent_stream() {
    let (db, _temp) = create_test_db();

    let result = db
        .get_stream("nonexistent", "transcript", "/path/to/transcript.jsonl")
        .unwrap();
    assert!(result.is_none());
}

#[test]
fn test_update_watermark() {
    let (db, _temp) = create_test_db();
    let stream = create_test_stream("session-1");
    db.insert_stream(&stream).unwrap();

    use crate::model::stream_watermark::ByteOffsetWatermark;
    let new_watermark = ByteOffsetWatermark::new(1234);

    db.update_watermark(
        "session-1",
        "transcript",
        "/path/to/transcript.jsonl",
        &new_watermark,
    )
    .unwrap();

    let retrieved = db
        .get_stream("session-1", "transcript", "/path/to/transcript.jsonl")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.watermark_value, "1234");
    assert!(retrieved.last_processed_at > stream.last_processed_at);
}

#[test]
fn test_update_file_metadata() {
    let (db, _temp) = create_test_db();
    let stream = create_test_stream("session-1");
    db.insert_stream(&stream).unwrap();

    let modified = Utc.with_ymd_and_hms(2024, 6, 15, 10, 30, 0).unwrap();
    db.update_file_metadata(
        "session-1",
        "transcript",
        "/path/to/transcript.jsonl",
        5678,
        Some(modified),
    )
    .unwrap();

    let retrieved = db
        .get_stream("session-1", "transcript", "/path/to/transcript.jsonl")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.last_known_size, 5678);
    assert_eq!(retrieved.last_modified, Some(modified.timestamp()));
}

#[test]
fn test_all_streams_empty() {
    let (db, _temp) = create_test_db();

    let streams = db.all_streams().unwrap();
    assert_eq!(streams.len(), 0);
}

#[test]
fn test_all_streams_multiple() {
    let (db, _temp) = create_test_db();

    let stream1 = create_test_stream("session-1");
    let stream2 = create_test_stream("session-2");
    let stream3 = create_test_stream("session-3");

    db.insert_stream(&stream1).unwrap();
    db.insert_stream(&stream2).unwrap();
    db.insert_stream(&stream3).unwrap();

    let streams = db.all_streams().unwrap();
    assert_eq!(streams.len(), 3);

    let ids: Vec<String> = streams.iter().map(|s| s.session_id.clone()).collect();
    assert!(ids.contains(&"session-1".to_string()));
    assert!(ids.contains(&"session-2".to_string()));
    assert!(ids.contains(&"session-3".to_string()));
}

#[test]
fn test_stream_with_nulls() {
    let (db, _temp) = create_test_db();

    let stream = StreamRecord {
        session_id: "session-null".to_string(),
        stream_kind: "transcript".to_string(),
        tool: "claude".to_string(),
        stream_path: "/path".to_string(),
        stream_format: StreamFormat::ClaudeJsonl,
        watermark_type: WatermarkType::ByteOffset,
        watermark_value: "0".to_string(),
        external_session_id: "session-null".to_string(),
        external_parent_session_id: None,
        first_seen_at: 1704067200,
        last_processed_at: 1704067200,
        last_known_size: 0,
        last_modified: None,
        processing_errors: 0,
        last_error: None,
        repo_work_dir: None,
    };

    db.insert_stream(&stream).unwrap();

    let retrieved = db
        .get_stream("session-null", "transcript", "/path")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.external_session_id, "session-null");
    assert_eq!(retrieved.last_modified, None);
    assert_eq!(retrieved.last_error, None);
    assert_eq!(retrieved.repo_work_dir, None);
}

#[test]
fn test_schema_version_tracking() {
    let temp_file = NamedTempFile::new().unwrap();
    let db = StreamsDatabase::open(temp_file.path()).unwrap();

    let conn = db.conn.lock().unwrap();
    let version: u32 = conn
        .query_row(
            "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 4); // Current schema version
}

#[test]
fn test_database_reopens_correctly() {
    let temp_file = NamedTempFile::new().unwrap();
    let path = temp_file.path().to_path_buf();

    {
        let db = StreamsDatabase::open(&path).unwrap();
        let stream = create_test_stream("session-1");
        db.insert_stream(&stream).unwrap();
    }

    // Reopen database
    let db = StreamsDatabase::open(&path).unwrap();
    let stream = db
        .get_stream("session-1", "transcript", "/path/to/transcript.jsonl")
        .unwrap();
    assert!(stream.is_some());
}

#[test]
fn test_indexes_created() {
    let temp_file = NamedTempFile::new().unwrap();
    let db = StreamsDatabase::open(temp_file.path()).unwrap();

    let conn = crate::model::repository::sqlite::poisoned_lock(&db.conn);
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name LIKE 'idx_streams_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 4); // 4 indexes defined in schema
}

#[test]
fn test_performance_pragmas_set() {
    let temp_file = NamedTempFile::new().unwrap();
    let db = StreamsDatabase::open(temp_file.path()).unwrap();

    let conn = crate::model::repository::sqlite::poisoned_lock(&db.conn);

    // synchronous returns an integer: 0=OFF, 1=NORMAL, 2=FULL, 3=EXTRA
    let synchronous: i32 = conn
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .unwrap();
    assert_eq!(synchronous, 1); // 1 = NORMAL

    let cache_size: i32 = conn
        .pragma_query_value(None, "cache_size", |row| row.get(0))
        .unwrap();
    assert_eq!(cache_size, -2000);

    // temp_store returns an integer: 0=DEFAULT, 1=FILE, 2=MEMORY
    let temp_store: i32 = conn
        .pragma_query_value(None, "temp_store", |row| row.get(0))
        .unwrap();
    assert_eq!(temp_store, 2); // 2 = MEMORY
}
