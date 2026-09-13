use super::*;
use crate::operations::streams::agents::test_support::{
    StreamAdapterContractCapabilities, StreamAdapterFixture, assert_stream_adapter_contract,
    drain_stream,
};

#[test]
fn test_sweep_strategy() {
    let agent = OpenCodeAgent::new();
    assert_eq!(
        agent.sweep_strategy(),
        SweepStrategy::Periodic(Duration::from_secs(30 * 60))
    );
}

fn create_test_db(path: &std::path::Path, message_count: usize) {
    if path.exists() {
        std::fs::remove_file(path).unwrap();
    }
    let conn = crate::model::repository::sqlite::open_with_memory_limits(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );",
    )
    .unwrap();
    append_test_records(&conn, 0, message_count);
}

fn append_test_records(conn: &rusqlite::Connection, first_record: usize, record_count: usize) {
    for i in first_record..first_record + record_count {
        let ts = 1000 + (i as i64) * 1000;
        conn.execute(
                "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    format!("msg-{}", i),
                    "test-session",
                    ts,
                    ts,
                    format!(r#"{{"role":"user","id":{}}}"#, i),
                ],
            ).unwrap();
        conn.execute(
                "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    format!("prt-{}", i),
                    format!("msg-{}", i),
                    "test-session",
                    ts + 1,
                    ts + 1,
                    format!(r#"{{"type":"text","text":"part-{}"}}"#, i),
                ],
            ).unwrap();
    }
}

#[test]
fn test_stream_adapter_contract() {
    use chrono::{DateTime, Utc};

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let reset_path = db_path.clone();
    let append_path = db_path.clone();
    let mut fixture = StreamAdapterFixture::new(
        &db_path,
        move |record_count| create_test_db(&reset_path, record_count),
        move |first_new_record, record_count| {
            let conn =
                crate::model::repository::sqlite::open_with_memory_limits(&append_path).unwrap();
            append_test_records(&conn, first_new_record, record_count);
        },
    );
    let agent = OpenCodeAgent::with_batch_size(2);
    assert_stream_adapter_contract(
        &agent,
        &mut fixture,
        || Box::new(TimestampWatermark::new(DateTime::<Utc>::UNIX_EPOCH)),
        |event| event["message"]["data"]["id"].as_u64().unwrap().to_string(),
        2,
        "test-session",
        StreamAdapterContractCapabilities::APPEND_ALL,
    );
}

#[test]
fn test_sqlite_open_sets_cache_size_pragma() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("opencode.db");
    drop(crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap());

    let conn = open_sqlite_readonly(&db_path).unwrap();

    let cache_size: i32 = conn
        .pragma_query_value(None, "cache_size", |row| row.get(0))
        .unwrap();
    assert_eq!(
        cache_size,
        crate::model::repository::sqlite::MEMORY_LIMIT_CACHE_SIZE_KIB
    );
}

#[test]
fn test_limit_caps_memory_and_watermark_still_drains_all() {
    use chrono::{DateTime, Utc};

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    create_test_db(&db_path, 20);

    // batch_size=3 forces multiple iterations to drain 20 messages
    let agent = OpenCodeAgent::with_batch_size(3);
    let (events, _) = drain_stream(
        &agent,
        &db_path,
        Box::new(TimestampWatermark::new(DateTime::<Utc>::UNIX_EPOCH)),
        3,
        "test-session",
    );

    assert_eq!(
        events.len(),
        20,
        "all 20 messages must be returned across batches"
    );
    let ids: Vec<u64> = events
        .iter()
        .map(|e| e["message"]["data"]["id"].as_u64().unwrap())
        .collect();
    let expected: Vec<u64> = (0..20).collect();
    assert_eq!(
        ids, expected,
        "messages must arrive in order with no gaps or duplicates"
    );
}

#[test]
fn test_limit_returns_at_most_batch_size_per_call() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    create_test_db(&db_path, 10);

    let agent = OpenCodeAgent::with_batch_size(4);
    let wm: Box<dyn WatermarkStrategy> = Box::new(TimestampWatermark::new(
        chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
    ));

    let batch = agent
        .read_incremental(&db_path, wm, "test-session")
        .unwrap();
    assert!(
        batch.events.len() <= 4,
        "single call must not exceed batch_size (got {})",
        batch.events.len()
    );
}

#[test]
fn test_parts_are_batch_loaded_not_per_message() {
    let db_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/opencode-sqlite/opencode.db");
    let conn = open_sqlite_readonly(&db_path).unwrap();
    // watermark=0 matches all messages in the fixture
    let parts = read_parts_for_messages_with_limit(&conn, "test-session-123", 0, 1000).unwrap();
    // Verify IN-subquery loading returns parts grouped by message_id.
    // Single query with IN-subquery instead of one per message,
    // prevents full-table-scan memory blowup on large unindexed databases.
    assert!(
        !parts.is_empty(),
        "batch parts query must return data from fixture"
    );
    for msg_parts in parts.values() {
        assert!(!msg_parts.is_empty());
    }
}

#[test]
fn test_extract_event_ids_with_tool_call() {
    let agent = OpenCodeAgent::new();
    let event = serde_json::json!({
        "message": {
            "id": "msg_c5d3ff79b001I77d7ERgcEhCCc",
            "session_id": "ses_3a2c00870ffebuyMGjJ2UiakYv",
            "time_created": 1000,
            "time_updated": 2000,
            "data": {
                "role": "assistant",
                "parentID": "msg_c5d3ff791001Egl5tW62x4Vgzo",
                "modelID": "big-pickle"
            }
        },
        "parts": [
            {
                "id": "prt_c5d4001ea001t4tNa4ACM94hno",
                "message_id": "msg_c5d3ff79b001I77d7ERgcEhCCc",
                "session_id": "ses_3a2c00870ffebuyMGjJ2UiakYv",
                "time_created": 1000,
                "time_updated": 2000,
                "data": {
                    "type": "tool",
                    "callID": "call_function_p43u37xcf94i_1",
                    "tool": "read",
                    "state": {"status": "completed"}
                }
            }
        ]
    });
    let (eid, pid, tid) = agent.extract_event_ids(&event);
    assert_eq!(eid, Some("msg_c5d3ff79b001I77d7ERgcEhCCc".to_string()));
    assert_eq!(pid, Some("msg_c5d3ff791001Egl5tW62x4Vgzo".to_string()));
    assert_eq!(tid, Some("call_function_p43u37xcf94i_1".to_string()));
}

#[test]
fn test_extract_event_ids_no_parts() {
    let agent = OpenCodeAgent::new();
    let event = serde_json::json!({
        "message": {
            "id": "msg_c5d3ff791001Egl5tW62x4Vgzo",
            "session_id": "ses_3a2c00870ffebuyMGjJ2UiakYv",
            "time_created": 1000,
            "time_updated": 1000,
            "data": {"role": "user"}
        }
    });
    let (eid, pid, tid) = agent.extract_event_ids(&event);
    assert_eq!(eid, Some("msg_c5d3ff791001Egl5tW62x4Vgzo".to_string()));
    assert_eq!(pid, None);
    assert_eq!(tid, None);
}

#[test]
fn test_extract_event_ids_with_parent_no_tool() {
    let agent = OpenCodeAgent::new();
    let event = serde_json::json!({
        "message": {
            "id": "msg_c5d400371001TvbvIzWZB1f9il",
            "session_id": "ses_3a2c00870ffebuyMGjJ2UiakYv",
            "time_created": 1000,
            "time_updated": 2000,
            "data": {
                "role": "assistant",
                "parentID": "msg_c5d3ff791001Egl5tW62x4Vgzo",
                "modelID": "big-pickle"
            }
        },
        "parts": [
            {
                "id": "prt_c5d4002f20016aBCkx6UdvIDBo",
                "message_id": "msg_c5d400371001TvbvIzWZB1f9il",
                "session_id": "ses_3a2c00870ffebuyMGjJ2UiakYv",
                "time_created": 1000,
                "time_updated": 2000,
                "data": {
                    "type": "step-finish",
                    "reason": "tool-calls",
                    "cost": 0
                }
            }
        ]
    });
    let (eid, pid, tid) = agent.extract_event_ids(&event);
    assert_eq!(eid, Some("msg_c5d400371001TvbvIzWZB1f9il".to_string()));
    assert_eq!(pid, Some("msg_c5d3ff791001Egl5tW62x4Vgzo".to_string()));
    assert_eq!(tid, None);
}
