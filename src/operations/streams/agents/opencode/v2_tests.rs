use super::*;
use crate::model::stream_watermark::TimestampCursorWatermark;
use crate::operations::streams::model_extraction::extract_model;

#[test]
fn v2_transcript_model_and_parent_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
    conn.execute_batch(r#"
        CREATE TABLE session_v2 (id TEXT PRIMARY KEY, parent_id TEXT, model TEXT);
        CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT, type TEXT, seq INTEGER, time_created INTEGER, time_updated INTEGER, data TEXT);
        INSERT INTO session_v2 VALUES ('s', 'parent', '{"id":"selected"}');
        INSERT INTO session_message VALUES ('a', 's', 'assistant', 1, 1000, 1000, '{"model":{"id":"producing"},"content":[{"type":"text","text":"hello"}]}');
        INSERT INTO session_message VALUES ('b', 's', 'user', 2, 1000, 1000, '{"content":[]}');
    "#).unwrap();
    assert_eq!(schema::parent(&conn, "s"), Some(Some("parent".into())));
    assert_eq!(
        extract_model(&path, StreamFormat::OpenCodeSqlite, Some("s")).unwrap(),
        Some("producing".into())
    );
    let agent = OpenCodeAgent::with_batch_size(1);
    let first = agent
        .read_incremental(&path, Box::new(TimestampCursorWatermark::initial()), "s")
        .unwrap();
    assert_eq!(
        first.events[0]["message"]["data"]["content"][0]["text"],
        "hello"
    );
    let second = agent
        .read_incremental(&path, first.new_watermark, "s")
        .unwrap();
    assert_eq!(second.events[0]["message"]["id"], "b");
    assert!(
        agent
            .read_incremental(&path, second.new_watermark, "s")
            .unwrap()
            .events
            .is_empty()
    );
}

#[test]
fn v2_separate_parts_and_oversize_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
    conn.execute_batch(r#"
        CREATE TABLE session_v2 (id TEXT PRIMARY KEY, parent_id TEXT, model TEXT);
        CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT, type TEXT, seq INTEGER, time_created INTEGER, time_updated INTEGER, data TEXT);
        CREATE TABLE session_part (ordinal INTEGER, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT, type TEXT, call_id TEXT);
        INSERT INTO session_v2 VALUES ('s', NULL, NULL);
        INSERT INTO session_message VALUES ('a','s','assistant',1,1000,1000,'{}');
        INSERT INTO session_part VALUES (0,'a','s',1000,1000,'{}','tool','call');
    "#).unwrap();
    let payload = serde_json::json!({"text": "x".repeat(crate::config::Config::get().max_transcript_line_bytes())}).to_string();
    conn.execute("UPDATE session_part SET data = ?1", [&payload])
        .unwrap();
    let agent = OpenCodeAgent::new();
    assert!(matches!(
        agent.read_incremental(&path, Box::new(TimestampCursorWatermark::initial()), "s"),
        Err(StreamError::Transient { .. })
    ));
    conn.execute("UPDATE session_part SET data = '{}'", [])
        .unwrap();
    let batch = agent
        .read_incremental(&path, Box::new(TimestampCursorWatermark::initial()), "s")
        .unwrap();
    assert_eq!(batch.events[0]["parts"][0]["call_id"], "call");
    assert_eq!(
        agent.extract_event_ids(&batch.events[0]).2.as_deref(),
        Some("call")
    );
}

#[test]
fn legacy_timestamp_replays_boundary_then_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    super::tests::create_test_db(&path, 3);
    let legacy = TimestampWatermark::new(chrono::DateTime::from_timestamp_millis(2000).unwrap());
    let agent = OpenCodeAgent::with_batch_size(1);
    let first = agent
        .read_incremental(&path, Box::new(legacy), "test-session")
        .unwrap();
    assert_eq!(first.events[0]["message"]["id"], "msg-1");
    let cursor = first
        .new_watermark
        .serialize()
        .parse::<TimestampCursorWatermark>()
        .unwrap();
    let next = agent
        .read_incremental(&path, Box::new(cursor), "test-session")
        .unwrap();
    assert_eq!(next.events[0]["message"]["id"], "msg-2");
}
