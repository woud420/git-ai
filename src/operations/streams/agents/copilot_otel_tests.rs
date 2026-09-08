use super::*;
use crate::model::stream_watermark::TimestampCursorWatermark;
use std::str::FromStr;

fn create_test_otel_db() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("traces.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    conn.execute_batch(
            "CREATE TABLE spans (
                span_id TEXT PRIMARY KEY, trace_id TEXT NOT NULL, parent_span_id TEXT,
                name TEXT NOT NULL, start_time_ms INTEGER NOT NULL, end_time_ms INTEGER NOT NULL,
                status_code INTEGER NOT NULL DEFAULT 0, status_message TEXT,
                operation_name TEXT, provider_name TEXT, agent_name TEXT, conversation_id TEXT,
                request_model TEXT, response_model TEXT,
                input_tokens INTEGER, output_tokens INTEGER, cached_tokens INTEGER, reasoning_tokens INTEGER,
                tool_name TEXT, tool_call_id TEXT, tool_type TEXT,
                chat_session_id TEXT, turn_index INTEGER, ttft_ms REAL
            );
            CREATE TABLE span_attributes (
                span_id TEXT NOT NULL REFERENCES spans(span_id) ON DELETE CASCADE,
                key TEXT NOT NULL, value TEXT,
                PRIMARY KEY (span_id, key)
            );
            CREATE TABLE span_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                span_id TEXT NOT NULL REFERENCES spans(span_id) ON DELETE CASCADE,
                name TEXT NOT NULL, timestamp_ms INTEGER NOT NULL, attributes TEXT
            );",
        )
        .unwrap();
    (dir, db_path)
}

fn insert_span(
    conn: &rusqlite::Connection,
    span_id: &str,
    end_time_ms: i64,
    input_tokens: i64,
    output_tokens: i64,
) {
    conn.execute(
            "INSERT INTO spans (span_id, trace_id, name, start_time_ms, end_time_ms, status_code, \
             operation_name, provider_name, request_model, response_model, input_tokens, output_tokens, chat_session_id) \
             VALUES (?1, 'trace1', 'chat gpt-4.1', ?2, ?3, 0, 'chat', 'github', 'gpt-4.1', 'gpt-4.1-2025-04-14', ?4, ?5, 'session1')",
            rusqlite::params![span_id, end_time_ms - 1000, end_time_ms, input_tokens, output_tokens],
        )
        .unwrap();
}

#[test]
fn test_empty_db_returns_empty_batch() {
    let (_dir, db_path) = create_test_otel_db();
    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&db_path, watermark, 100).unwrap();
    assert!(batch.events.is_empty());
}

#[test]
fn test_reads_spans_after_watermark() {
    let (_dir, db_path) = create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    insert_span(&conn, "span1", 1000, 100, 50);
    insert_span(&conn, "span2", 2000, 200, 100);
    insert_span(&conn, "span3", 3000, 300, 150);
    drop(conn);

    let watermark: Box<dyn WatermarkStrategy> =
        Box::new(TimestampCursorWatermark::new(1000.0, "span1".to_string()));
    let batch = read_otel_spans_incremental(&db_path, watermark, 100).unwrap();
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.events[0]["span"]["span_id"], "span2");
    assert_eq!(batch.events[1]["span"]["span_id"], "span3");
}

#[test]
fn test_batch_size_limits_results() {
    let (_dir, db_path) = create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    for i in 1..=5 {
        insert_span(&conn, &format!("span{}", i), i * 1000, i * 100, i * 50);
    }
    drop(conn);

    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&db_path, watermark, 3).unwrap();
    assert_eq!(batch.events.len(), 3);
}

#[test]
fn test_batch_resume_no_loss_no_repeats() {
    let (_dir, db_path) = create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    for i in 1..=5 {
        insert_span(&conn, &format!("span{}", i), i * 1000, i * 100, i * 50);
    }
    drop(conn);

    let mut watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let mut all_ids = Vec::new();

    loop {
        let batch = read_otel_spans_incremental(&db_path, watermark, 2).unwrap();
        if batch.events.is_empty() {
            break;
        }
        for ev in &batch.events {
            all_ids.push(ev["span"]["span_id"].as_str().unwrap().to_string());
        }
        watermark = batch.new_watermark;
    }

    assert_eq!(all_ids, vec!["span1", "span2", "span3", "span4", "span5"]);
}

#[test]
fn test_no_data_loss_with_duplicate_end_time_ms() {
    let (_dir, db_path) = create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    // 5 spans all sharing the same end_time_ms
    for i in 1..=5 {
        insert_span(&conn, &format!("span{}", i), 3000, i * 100, i * 50);
    }
    drop(conn);

    let mut watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let mut all_ids = Vec::new();

    loop {
        let batch = read_otel_spans_incremental(&db_path, watermark, 2).unwrap();
        if batch.events.is_empty() {
            break;
        }
        for ev in &batch.events {
            all_ids.push(ev["span"]["span_id"].as_str().unwrap().to_string());
        }
        watermark = batch.new_watermark;
    }

    assert_eq!(all_ids, vec!["span1", "span2", "span3", "span4", "span5"]);
}

#[test]
fn test_attributes_denormalized() {
    let (_dir, db_path) = create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    insert_span(&conn, "span1", 1000, 100, 50);
    conn.execute(
            "INSERT INTO span_attributes (span_id, key, value) VALUES ('span1', 'gen_ai.request.model', 'gpt-4.1')",
            [],
        )
        .unwrap();
    conn.execute(
            "INSERT INTO span_attributes (span_id, key, value) VALUES ('span1', 'gen_ai.agent.name', 'copilot')",
            [],
        )
        .unwrap();
    drop(conn);

    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&db_path, watermark, 100).unwrap();
    assert_eq!(
        batch.events[0]["attributes"]["gen_ai.request.model"],
        "gpt-4.1"
    );
    assert_eq!(
        batch.events[0]["attributes"]["gen_ai.agent.name"],
        "copilot"
    );
}

#[test]
fn test_span_events_denormalized() {
    let (_dir, db_path) = create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    insert_span(&conn, "span1", 1000, 100, 50);
    conn.execute(
            "INSERT INTO span_events (span_id, name, timestamp_ms, attributes) VALUES ('span1', 'tool_call', 500, '{\"tool\":\"read_file\"}')",
            [],
        )
        .unwrap();
    drop(conn);

    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&db_path, watermark, 100).unwrap();
    let events = batch.events[0]["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["name"], "tool_call");
    assert_eq!(events[0]["timestamp_ms"], 500);
    assert_eq!(events[0]["attributes"]["tool"], "read_file");
}

#[test]
fn test_extract_event_ids() {
    let event = serde_json::json!({
        "span": {
            "span_id": "abc123",
            "parent_span_id": "parent456",
            "tool_call_id": "call789",
        },
        "attributes": {},
        "events": [],
    });
    let (event_id, parent_id, tool_use_id) = extract_otel_event_ids(&event);
    assert_eq!(event_id, Some("abc123".to_string()));
    assert_eq!(parent_id, Some("parent456".to_string()));
    assert_eq!(tool_use_id, Some("call789".to_string()));
}

#[test]
fn test_extract_event_ids_empty_tool_call_id() {
    let event = serde_json::json!({
        "span": { "span_id": "abc", "parent_span_id": null, "tool_call_id": "" },
        "attributes": {},
        "events": [],
    });
    let (event_id, parent_id, tool_use_id) = extract_otel_event_ids(&event);
    assert_eq!(event_id, Some("abc".to_string()));
    assert_eq!(parent_id, None);
    assert_eq!(tool_use_id, None);
}

#[test]
fn test_extract_event_timestamp() {
    let event = serde_json::json!({
        "span": { "start_time_ms": 1716556800000_i64 },
    });
    let ts = extract_otel_event_timestamp(&event);
    assert_eq!(ts, Some(1716556800));
}

#[test]
fn test_reads_from_real_fixture() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/copilot-otel/traces.db");
    if !fixture_path.exists() {
        return;
    }
    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&fixture_path, watermark, 100).unwrap();
    assert!(!batch.events.is_empty());
    let first = &batch.events[0];
    assert!(first.get("span").is_some());
    assert!(first.get("attributes").is_some());
    assert!(first.get("events").is_some());
    // Verify token fields are present
    assert!(first["span"].get("input_tokens").is_some());
    assert!(first["span"].get("output_tokens").is_some());
}

#[test]
fn test_spans_without_session_ids_are_filtered() {
    let (_dir, db_path) = create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();

    // Span WITH session ID (should be included)
    conn.execute(
        "INSERT INTO spans (span_id, trace_id, name, start_time_ms, end_time_ms, status_code, \
             chat_session_id, conversation_id) \
             VALUES ('has-session', 'trace1', 'chat', 1000, 2000, 0, 'sess-1', NULL)",
        [],
    )
    .unwrap();

    // Span with only conversation_id (should be included)
    conn.execute(
        "INSERT INTO spans (span_id, trace_id, name, start_time_ms, end_time_ms, status_code, \
             chat_session_id, conversation_id) \
             VALUES ('has-conv-only', 'trace1', 'chat', 2000, 3000, 0, NULL, 'conv-1')",
        [],
    )
    .unwrap();

    // Span WITHOUT any session ID (should be excluded by SQL filter)
    conn.execute(
        "INSERT INTO spans (span_id, trace_id, name, start_time_ms, end_time_ms, status_code, \
             chat_session_id, conversation_id) \
             VALUES ('no-session', 'trace1', 'chat', 3000, 4000, 0, NULL, NULL)",
        [],
    )
    .unwrap();

    // Span with empty strings (should be excluded)
    conn.execute(
        "INSERT INTO spans (span_id, trace_id, name, start_time_ms, end_time_ms, status_code, \
             chat_session_id, conversation_id) \
             VALUES ('empty-session', 'trace1', 'chat', 4000, 5000, 0, '', '')",
        [],
    )
    .unwrap();

    drop(conn);

    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&db_path, watermark, 100).unwrap();

    assert_eq!(
        batch.events.len(),
        2,
        "only spans with session IDs should be returned"
    );
    let ids: Vec<&str> = batch
        .events
        .iter()
        .map(|e| e["span"]["span_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"has-session"));
    assert!(ids.contains(&"has-conv-only"));
}

#[test]
fn test_watermark_advances_correctly_after_batch() {
    let (_dir, db_path) = create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    insert_span(&conn, "span-a", 5000, 100, 50);
    insert_span(&conn, "span-b", 7000, 200, 100);
    drop(conn);

    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&db_path, watermark, 100).unwrap();

    // Watermark should point to last span
    let new_wm = batch
        .new_watermark
        .as_any()
        .downcast_ref::<TimestampCursorWatermark>()
        .unwrap();
    assert_eq!(new_wm.timestamp_millis, 7000.0);
    assert_eq!(new_wm.last_id, "span-b");
}

#[test]
fn test_map_sqlite_error_busy_is_transient() {
    let err = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ffi::ErrorCode::DatabaseBusy,
            extended_code: 5,
        },
        Some("database is locked".to_string()),
    );
    let result = super::map_sqlite_error(err, "test operation");
    match result {
        StreamError::Transient {
            message,
            retry_after,
        } => {
            assert!(message.contains("test operation"));
            assert!(message.contains("database is locked"));
            assert_eq!(retry_after, Duration::from_secs(2));
        }
        other => panic!("Expected Transient, got {:?}", other),
    }
}

#[test]
fn test_map_sqlite_error_other_is_fatal() {
    let err = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ffi::ErrorCode::DatabaseCorrupt,
            extended_code: 11,
        },
        Some("database disk image is malformed".to_string()),
    );
    let result = super::map_sqlite_error(err, "test operation");
    match result {
        StreamError::Fatal { message } => {
            assert!(message.contains("test operation"));
            assert!(message.contains("malformed"));
        }
        other => panic!("Expected Fatal, got {:?}", other),
    }
}

#[test]
fn test_otel_json_structure_has_all_span_fields() {
    let (_dir, db_path) = create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    conn.execute(
        "INSERT INTO spans (span_id, trace_id, parent_span_id, name, start_time_ms, end_time_ms, \
             status_code, status_message, operation_name, provider_name, agent_name, \
             conversation_id, request_model, response_model, input_tokens, output_tokens, \
             cached_tokens, reasoning_tokens, tool_name, tool_call_id, tool_type, \
             chat_session_id, turn_index, ttft_ms) \
             VALUES ('full-span', 'trace-abc', 'parent-123', 'chat gpt-4.1', 1000, 2000, \
             1, 'OK', 'chat', 'openai', 'copilot-agent', \
             'conv-1', 'gpt-4.1', 'gpt-4.1-2025-04-14', 500, 200, \
             100, 50, 'read_file', 'call-xyz', 'function', \
             'session-abc', 3, 125.5)",
        [],
    )
    .unwrap();
    drop(conn);

    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&db_path, watermark, 100).unwrap();
    assert_eq!(batch.events.len(), 1);

    let span = &batch.events[0]["span"];
    assert_eq!(span["span_id"], "full-span");
    assert_eq!(span["trace_id"], "trace-abc");
    assert_eq!(span["parent_span_id"], "parent-123");
    assert_eq!(span["name"], "chat gpt-4.1");
    assert_eq!(span["start_time_ms"], 1000);
    assert_eq!(span["end_time_ms"], 2000);
    assert_eq!(span["status_code"], 1);
    assert_eq!(span["status_message"], "OK");
    assert_eq!(span["operation_name"], "chat");
    assert_eq!(span["provider_name"], "openai");
    assert_eq!(span["agent_name"], "copilot-agent");
    assert_eq!(span["conversation_id"], "conv-1");
    assert_eq!(span["request_model"], "gpt-4.1");
    assert_eq!(span["response_model"], "gpt-4.1-2025-04-14");
    assert_eq!(span["input_tokens"], 500);
    assert_eq!(span["output_tokens"], 200);
    assert_eq!(span["cached_tokens"], 100);
    assert_eq!(span["reasoning_tokens"], 50);
    assert_eq!(span["tool_name"], "read_file");
    assert_eq!(span["tool_call_id"], "call-xyz");
    assert_eq!(span["tool_type"], "function");
    assert_eq!(span["chat_session_id"], "session-abc");
    assert_eq!(span["turn_index"], 3);
    assert_eq!(span["ttft_ms"], 125.5);
}

#[test]
fn test_initial_watermark_uses_simple_greater_than() {
    let (_dir, db_path) = create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    // Span at time 0 should still be found with initial watermark (end_time_ms > 0 fails for this!)
    // Actually initial watermark is timestamp_millis=0, so end_time_ms > 0 catches spans at ms=1+
    // Span at ms=0 would NOT be found since > 0 excludes it. This is OK since ms=0 means epoch.
    insert_span(&conn, "span-early", 1, 10, 5);
    insert_span(&conn, "span-at-zero", 0, 10, 5);
    drop(conn);

    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&db_path, watermark, 100).unwrap();
    // span-at-zero has end_time_ms=0, initial watermark is > 0, so it's excluded
    // However span-at-zero also needs a session ID to pass the filter
    // Our insert_span helper sets chat_session_id='session1', so it passes the session filter
    // But end_time_ms=0 is NOT > 0, so it's excluded from the initial query
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.events[0]["span"]["span_id"], "span-early");
}

#[test]
fn test_fractional_real_end_time_ms_no_infinite_loop() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("traces.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    conn.execute_batch(
            "CREATE TABLE spans (
                span_id TEXT PRIMARY KEY, trace_id TEXT NOT NULL, parent_span_id TEXT,
                name TEXT NOT NULL, start_time_ms REAL NOT NULL, end_time_ms REAL NOT NULL,
                status_code INTEGER NOT NULL DEFAULT 0, status_message TEXT,
                operation_name TEXT, provider_name TEXT, agent_name TEXT, conversation_id TEXT,
                request_model TEXT, response_model TEXT,
                input_tokens INTEGER, output_tokens INTEGER, cached_tokens INTEGER, reasoning_tokens INTEGER,
                tool_name TEXT, tool_call_id TEXT, tool_type TEXT,
                chat_session_id TEXT, turn_index INTEGER, ttft_ms REAL
            );
            CREATE TABLE span_attributes (
                span_id TEXT NOT NULL REFERENCES spans(span_id) ON DELETE CASCADE,
                key TEXT NOT NULL, value TEXT,
                PRIMARY KEY (span_id, key)
            );
            CREATE TABLE span_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                span_id TEXT NOT NULL REFERENCES spans(span_id) ON DELETE CASCADE,
                name TEXT NOT NULL, timestamp_ms INTEGER NOT NULL, attributes TEXT
            );",
        )
        .unwrap();

    conn.execute(
        "INSERT INTO spans (span_id, trace_id, name, start_time_ms, end_time_ms, \
             status_code, chat_session_id) \
             VALUES ('span1', 'trace1', 'chat', 1000.16, 2000.35, 0, 'session1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO spans (span_id, trace_id, name, start_time_ms, end_time_ms, \
             status_code, chat_session_id) \
             VALUES ('span2', 'trace1', 'chat', 2000.50, 3000.94, 0, 'session1')",
        [],
    )
    .unwrap();
    drop(conn);

    // First read: get both spans
    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&db_path, watermark, 100).unwrap();
    assert_eq!(batch.events.len(), 2);

    // Watermark should store the exact fractional value
    let new_wm = batch
        .new_watermark
        .as_any()
        .downcast_ref::<TimestampCursorWatermark>()
        .unwrap();
    assert_eq!(new_wm.timestamp_millis, 3000.94);
    assert_eq!(new_wm.last_id, "span2");

    // Second read with advanced watermark: should get 0 spans (not loop forever)
    let batch2 = read_otel_spans_incremental(&db_path, batch.new_watermark, 100).unwrap();
    assert_eq!(batch2.events.len(), 0);
}

#[test]
fn test_fractional_real_watermark_roundtrip_prevents_reread() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("traces.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    conn.execute_batch(
            "CREATE TABLE spans (
                span_id TEXT PRIMARY KEY, trace_id TEXT NOT NULL, parent_span_id TEXT,
                name TEXT NOT NULL, start_time_ms REAL NOT NULL, end_time_ms REAL NOT NULL,
                status_code INTEGER NOT NULL DEFAULT 0, status_message TEXT,
                operation_name TEXT, provider_name TEXT, agent_name TEXT, conversation_id TEXT,
                request_model TEXT, response_model TEXT,
                input_tokens INTEGER, output_tokens INTEGER, cached_tokens INTEGER, reasoning_tokens INTEGER,
                tool_name TEXT, tool_call_id TEXT, tool_type TEXT,
                chat_session_id TEXT, turn_index INTEGER, ttft_ms REAL
            );
            CREATE TABLE span_attributes (
                span_id TEXT NOT NULL REFERENCES spans(span_id) ON DELETE CASCADE,
                key TEXT NOT NULL, value TEXT,
                PRIMARY KEY (span_id, key)
            );
            CREATE TABLE span_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                span_id TEXT NOT NULL REFERENCES spans(span_id) ON DELETE CASCADE,
                name TEXT NOT NULL, timestamp_ms INTEGER NOT NULL, attributes TEXT
            );",
        )
        .unwrap();

    conn.execute(
            "INSERT INTO spans (span_id, trace_id, name, start_time_ms, end_time_ms, \
             status_code, chat_session_id) \
             VALUES ('span-frac', 'trace1', 'chat', 1780519329188.16, 1780519329188.35, 0, 'session1')",
            [],
        )
        .unwrap();
    drop(conn);

    // Read the span
    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = read_otel_spans_incremental(&db_path, watermark, 100).unwrap();
    assert_eq!(batch.events.len(), 1);

    // Simulate serialization roundtrip (as would happen when persisted to streams DB)
    let wm = batch
        .new_watermark
        .as_any()
        .downcast_ref::<TimestampCursorWatermark>()
        .unwrap();
    let serialized = wm.serialize();
    let deserialized = TimestampCursorWatermark::from_str(&serialized).unwrap();
    assert_eq!(deserialized.timestamp_millis, 1780519329188.35);

    // Re-read with roundtripped watermark: must NOT re-read the same span
    let restored_wm: Box<dyn WatermarkStrategy> = Box::new(deserialized);
    let batch2 = read_otel_spans_incremental(&db_path, restored_wm, 100).unwrap();
    assert_eq!(batch2.events.len(), 0);
}
