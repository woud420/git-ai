use super::{
    ByteOffsetWatermark, ClaudeAgent, File, StreamFormat, StreamRecord, StreamsDatabase, TempDir,
    WatermarkType,
};
use git_ai::operations::streams::agent::Agent;
use std::fs;
use std::io::Write;

#[test]
fn test_session_database_basic() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("transcripts.db");
    let db = StreamsDatabase::open(&db_path).unwrap();

    let now = chrono::Utc::now().timestamp();
    let session = StreamRecord {
        session_id: "s_test_123".to_string(),
        stream_kind: "transcript".to_string(),
        tool: "claude".to_string(),
        stream_path: "/path/to/transcript.jsonl".to_string(),
        stream_format: StreamFormat::ClaudeJsonl,
        watermark_type: WatermarkType::ByteOffset,
        watermark_value: "0".to_string(),
        external_session_id: "test-ext-session".to_string(),
        external_parent_session_id: None,
        first_seen_at: now,
        last_processed_at: now,
        last_known_size: 0,
        last_modified: None,
        processing_errors: 0,
        last_error: None,
        repo_work_dir: None,
    };

    // Insert
    db.insert_stream(&session).unwrap();

    // Read
    let retrieved = db
        .get_stream("s_test_123", "transcript", "/path/to/transcript.jsonl")
        .unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.session_id, "s_test_123");
    assert_eq!(retrieved.tool, "claude");
    assert_eq!(retrieved.processing_errors, 0);

    // Update watermark
    let new_watermark = ByteOffsetWatermark::new(100);
    db.update_watermark(
        "s_test_123",
        "transcript",
        "/path/to/transcript.jsonl",
        &new_watermark,
    )
    .unwrap();
    let retrieved_updated = db
        .get_stream("s_test_123", "transcript", "/path/to/transcript.jsonl")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved_updated.watermark_value, "100");

    // List all sessions
    let all_sessions = db.all_streams().unwrap();
    assert_eq!(all_sessions.len(), 1);
    assert_eq!(all_sessions[0].session_id, "s_test_123");
}

#[test]
fn test_watermark_integration() {
    let temp_dir = TempDir::new().unwrap();
    let transcript_file = temp_dir.path().join("watermark_test.jsonl");

    // Write initial content
    let mut file = File::create(&transcript_file).unwrap();
    writeln!(
        file,
        r#"{{"type":"user","message":{{"content":"First"}},"timestamp":"2025-01-01T00:00:00Z"}}"#
    )
    .unwrap();
    file.flush().unwrap();
    drop(file);

    // Read from start
    let agent = ClaudeAgent::new();
    let watermark1 = Box::new(ByteOffsetWatermark::new(0));
    let result1 = agent
        .read_incremental(&transcript_file, watermark1, "s_test")
        .unwrap();
    assert_eq!(result1.events.len(), 1);

    let offset1: u64 = result1.new_watermark.serialize().parse().unwrap();
    assert!(offset1 > 0, "Watermark should advance");

    // Append more content
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&transcript_file)
        .unwrap();
    writeln!(
        file,
        r#"{{"type":"user","message":{{"content":"Second"}},"timestamp":"2025-01-01T00:00:01Z"}}"#
    )
    .unwrap();
    file.flush().unwrap();
    drop(file);

    // Read from watermark - should only get new line
    let watermark2 = Box::new(ByteOffsetWatermark::new(offset1));
    let result2 = agent
        .read_incremental(&transcript_file, watermark2, "s_test")
        .unwrap();
    assert_eq!(result2.events.len(), 1);
    assert_eq!(
        result2.events[0]["message"]["content"].as_str(),
        Some("Second")
    );

    let offset2: u64 = result2.new_watermark.serialize().parse().unwrap();
    assert!(offset2 > offset1, "Watermark should continue advancing");
}

#[test]
fn test_multiple_sessions_isolation() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("transcripts.db");
    let db = StreamsDatabase::open(&db_path).unwrap();

    let now = chrono::Utc::now().timestamp();

    // Create multiple sessions
    for i in 0..5 {
        let session = StreamRecord {
            session_id: format!("s_session_{}", i),
            stream_kind: "transcript".to_string(),
            tool: "claude".to_string(),
            stream_path: format!("/path/to/transcript_{}.jsonl", i),
            stream_format: StreamFormat::ClaudeJsonl,
            watermark_type: WatermarkType::ByteOffset,
            watermark_value: (i * 10).to_string(),
            external_session_id: "test-ext-session".to_string(),
            external_parent_session_id: None,
            first_seen_at: now,
            last_processed_at: now,
            last_known_size: 0,
            last_modified: None,
            processing_errors: 0,
            last_error: None,
            repo_work_dir: None,
        };
        db.insert_stream(&session).unwrap();
    }

    // Verify all sessions exist independently
    let all_sessions = db.all_streams().unwrap();
    assert_eq!(all_sessions.len(), 5);

    // Verify each session has correct data
    for i in 0..5 {
        let session = db
            .get_stream(
                &format!("s_session_{}", i),
                "transcript",
                &format!("/path/to/transcript_{}.jsonl", i),
            )
            .unwrap()
            .unwrap();
        assert_eq!(session.watermark_value, (i * 10).to_string());
        assert_eq!(session.processing_errors, 0);
    }
}

#[test]
fn test_database_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("transcripts.db");

    let now = chrono::Utc::now().timestamp();

    // Create and close database
    {
        let db = StreamsDatabase::open(&db_path).unwrap();
        let session = StreamRecord {
            session_id: "s_persist".to_string(),
            stream_kind: "transcript".to_string(),
            tool: "claude".to_string(),
            stream_path: "/path/to/transcript.jsonl".to_string(),
            stream_format: StreamFormat::ClaudeJsonl,
            watermark_type: WatermarkType::ByteOffset,
            watermark_value: "42".to_string(),
            external_session_id: "test-ext-session".to_string(),
            external_parent_session_id: None,
            first_seen_at: now,
            last_processed_at: now,
            last_known_size: 0,
            last_modified: None,
            processing_errors: 0,
            last_error: None,
            repo_work_dir: None,
        };
        db.insert_stream(&session).unwrap();
    }

    // Reopen database
    {
        let db = StreamsDatabase::open(&db_path).unwrap();
        let retrieved = db
            .get_stream("s_persist", "transcript", "/path/to/transcript.jsonl")
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.session_id, "s_persist");
        assert_eq!(retrieved.watermark_value, "42");
        assert_eq!(retrieved.processing_errors, 0);
    }
}

#[test]
fn test_error_tracking() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("transcripts.db");
    let db = StreamsDatabase::open(&db_path).unwrap();

    let now = chrono::Utc::now().timestamp();
    let session = StreamRecord {
        session_id: "s_errors".to_string(),
        stream_kind: "transcript".to_string(),
        tool: "claude".to_string(),
        stream_path: "/path/to/transcript.jsonl".to_string(),
        stream_format: StreamFormat::ClaudeJsonl,
        watermark_type: WatermarkType::ByteOffset,
        watermark_value: "0".to_string(),
        external_session_id: "test-ext-session".to_string(),
        external_parent_session_id: None,
        first_seen_at: now,
        last_processed_at: now,
        last_known_size: 0,
        last_modified: None,
        processing_errors: 0,
        last_error: None,
        repo_work_dir: None,
    };

    db.insert_stream(&session).unwrap();

    // Simulate errors
    db.record_error(
        "s_errors",
        "transcript",
        "/path/to/transcript.jsonl",
        "First error",
    )
    .unwrap();
    let retrieved = db
        .get_stream("s_errors", "transcript", "/path/to/transcript.jsonl")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.processing_errors, 1);
    assert_eq!(retrieved.last_error, Some("First error".to_string()));

    // More errors
    db.record_error(
        "s_errors",
        "transcript",
        "/path/to/transcript.jsonl",
        "Second error",
    )
    .unwrap();
    let retrieved2 = db
        .get_stream("s_errors", "transcript", "/path/to/transcript.jsonl")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved2.processing_errors, 2);
    assert_eq!(retrieved2.last_error, Some("Second error".to_string()));
}
