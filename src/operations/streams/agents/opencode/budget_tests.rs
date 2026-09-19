use super::*;
use crate::config::Config;
use chrono::{DateTime, Utc};

fn initial() -> Box<dyn WatermarkStrategy> {
    Box::new(TimestampWatermark::new(DateTime::<Utc>::UNIX_EPOCH))
}

#[test]
fn tied_timestamp_messages_are_never_split_at_the_row_hint() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    super::tests::create_test_db(&path, 4);
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
    conn.execute(
        "UPDATE message SET time_updated = 1000 WHERE id != 'msg-3'",
        [],
    )
    .unwrap();
    let agent = OpenCodeAgent::with_batch_size(2);
    let first = agent
        .read_incremental(&path, initial(), "test-session")
        .unwrap();
    assert_eq!(
        first.events.len(),
        3,
        "the timestamp watermark cannot represent a partial group"
    );
    let second = agent
        .read_incremental(&path, first.new_watermark, "test-session")
        .unwrap();
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.events[0]["message"]["id"], "msg-3");
}

#[test]
fn oversized_unicode_part_blocks_only_after_the_complete_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    super::tests::create_test_db(&path, 3);
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
    let oversized =
        serde_json::json!({"text": "界".repeat(Config::get().max_transcript_line_bytes() / 2)})
            .to_string();
    conn.execute(
        "UPDATE part SET data = ?1 WHERE message_id = 'msg-1'",
        [&oversized],
    )
    .unwrap();
    let agent = OpenCodeAgent::new();
    let first = agent
        .read_incremental(&path, initial(), "test-session")
        .unwrap();
    assert_eq!(
        first.events.len(),
        1,
        "payload bytes, including parts, must bound the batch"
    );
    assert_eq!(first.events[0]["message"]["id"], "msg-0");
    let cursor = first.new_watermark.serialize();
    let blocked = agent.read_incremental(&path, first.new_watermark, "test-session");
    assert!(matches!(blocked, Err(StreamError::Transient { .. })));
    conn.execute("UPDATE part SET data = '{}' WHERE message_id = 'msg-1'", [])
        .unwrap();
    let resume = cursor.parse::<TimestampWatermark>().unwrap();
    let resumed = agent
        .read_incremental(&path, Box::new(resume), "test-session")
        .unwrap();
    assert_eq!(resumed.events.len(), 2);
    assert_eq!(resumed.events[0]["message"]["id"], "msg-1");
    assert_eq!(resumed.events[1]["message"]["id"], "msg-2");
}

#[test]
fn oversized_message_does_not_bypass_the_budget_through_model_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    super::tests::create_test_db(&path, 1);
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
    let oversized = serde_json::json!({"modelID":"large-model", "text":"界".repeat(Config::get().max_transcript_line_bytes() / 2)}).to_string();
    conn.execute("UPDATE message SET data = ?1", [&oversized])
        .unwrap();
    assert_eq!(
        crate::operations::streams::model_extraction::extract_model(
            &path,
            crate::operations::streams::sweep::StreamFormat::OpenCodeSqlite,
            Some("test-session")
        )
        .unwrap(),
        None
    );
}

#[test]
fn batch_bytes_stop_before_a_whole_tied_group_and_resume_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    super::tests::create_test_db(&path, 3);
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
    let payload = serde_json::json!({"text": "x".repeat(Config::get().max_transcript_batch_bytes() / 2 - 512)}).to_string();
    conn.execute("UPDATE part SET data = ?1", [&payload])
        .unwrap();
    conn.execute(
        "UPDATE message SET time_updated = 2000 WHERE id != 'msg-0'",
        [],
    )
    .unwrap();
    let agent = OpenCodeAgent::new();
    let first = agent
        .read_incremental(&path, initial(), "test-session")
        .unwrap();
    assert_eq!(first.events.len(), 1);
    let second = agent
        .read_incremental(&path, first.new_watermark, "test-session")
        .unwrap();
    assert_eq!(second.events.len(), 2);
    assert_eq!(second.events[0]["message"]["id"], "msg-1");
    assert_eq!(second.events[1]["message"]["id"], "msg-2");
    let last = agent
        .read_incremental(&path, second.new_watermark, "test-session")
        .unwrap();
    assert!(last.events.is_empty());
    conn.execute("UPDATE message SET time_updated = 1000", [])
        .unwrap();
    assert!(matches!(
        agent.read_incremental(&path, initial(), "test-session"),
        Err(StreamError::Transient { .. })
    ));
}

#[test]
fn the_read_snapshot_keeps_parts_from_growing_after_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    super::tests::create_test_db(&path, 1);
    let writer = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    let mut reader = open_sqlite_readonly(&path).unwrap();
    let tx = reader.transaction().unwrap();
    let through = super::budget::plan_through(&tx, "test-session", 0, 1000)
        .unwrap()
        .unwrap();
    let oversized =
        serde_json::json!({"text": "x".repeat(Config::get().max_transcript_line_bytes())})
            .to_string();
    writer
        .execute("UPDATE part SET data = ?1", [&oversized])
        .unwrap();
    let parts =
        super::reader::read_parts_for_messages_through(&tx, "test-session", 0, through).unwrap();
    assert_eq!(parts["msg-0"][0]["data"]["text"], "part-0");
    drop(tx);
    assert!(matches!(
        OpenCodeAgent::new().read_incremental(&path, initial(), "test-session"),
        Err(StreamError::Transient { .. })
    ));
}
