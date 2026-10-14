use super::*;
use crate::config::Config;

#[test]
fn oversized_otel_payloads_leave_the_keyset_cursor_before_the_event() {
    for source in ["span", "attribute", "event"] {
        let (_dir, path) = super::tests::create_test_otel_db();
        let conn = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
        for i in 1..=3 {
            super::tests::insert_span(&conn, &format!("span{i}"), 1000, 1, 1);
        }
        let large = "界".repeat(Config::get().max_transcript_line_bytes() / 2);
        match source {
            "span" => {
                conn.execute(
                    "UPDATE spans SET name = ?1 WHERE span_id = 'span2'",
                    [&large],
                )
                .unwrap();
            }
            "attribute" => {
                conn.execute(
                    "INSERT INTO span_attributes VALUES ('span2', 'payload', ?1)",
                    [&large],
                )
                .unwrap();
            }
            _ => {
                conn.execute("INSERT INTO span_events (span_id, name, timestamp_ms, attributes) VALUES ('span2', 'event', 1000, ?1)", [serde_json::json!({"text":large}).to_string()]).unwrap();
            }
        }
        let first =
            read_otel_spans_incremental(&path, Box::new(TimestampCursorWatermark::initial()), 100)
                .unwrap();
        assert_eq!(
            first.events.len(),
            1,
            "{source} bytes must be included before materialization"
        );
        assert_eq!(first.events[0]["span"]["span_id"], "span1");
        let cursor = first.new_watermark.serialize();
        assert!(matches!(
            read_otel_spans_incremental(&path, first.new_watermark, 100),
            Err(StreamError::Transient { .. })
        ));
        conn.execute(
            "UPDATE spans SET name = 'fixed' WHERE span_id = 'span2'",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM span_attributes", []).unwrap();
        conn.execute("DELETE FROM span_events", []).unwrap();
        let cursor: TimestampCursorWatermark = cursor.parse().unwrap();
        let resumed = read_otel_spans_incremental(&path, Box::new(cursor), 100).unwrap();
        assert_eq!(resumed.events.len(), 2);
        assert_eq!(resumed.events[0]["span"]["span_id"], "span2");
        assert_eq!(resumed.events[1]["span"]["span_id"], "span3");
    }
}

#[test]
fn otel_batch_bytes_resume_within_a_tied_timestamp() {
    let (_dir, path) = super::tests::create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
    let payload = "x".repeat(Config::get().max_transcript_batch_bytes() / 2 - 512);
    for i in 1..=3 {
        let id = format!("span{i}");
        super::tests::insert_span(&conn, &id, 1000, 1, 1);
        conn.execute(
            "INSERT INTO span_attributes VALUES (?1, 'payload', ?2)",
            [&id, &payload],
        )
        .unwrap();
    }
    let first =
        read_otel_spans_incremental(&path, Box::new(TimestampCursorWatermark::initial()), 100)
            .unwrap();
    assert_eq!(first.events.len(), 2);
    let second = read_otel_spans_incremental(&path, first.new_watermark, 100).unwrap();
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.events[0]["span"]["span_id"], "span3");
}

#[test]
fn oversized_otel_model_does_not_bypass_the_read_budget() {
    let (_dir, path) = super::tests::create_test_otel_db();
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
    super::tests::insert_span(&conn, "span1", 1000, 1, 1);
    conn.execute(
        "UPDATE spans SET request_model = ?1, response_model = NULL",
        ["x".repeat(Config::get().max_transcript_line_bytes() + 1)],
    )
    .unwrap();
    assert_eq!(
        crate::operations::streams::model_extraction::extract_model(
            &path,
            crate::operations::streams::sweep::StreamFormat::CopilotOtelSqlite,
            Some("session1")
        )
        .unwrap(),
        None
    );
}

#[test]
fn otel_child_rows_share_the_snapshot_used_by_the_byte_plan() {
    let (_dir, path) = super::tests::create_test_otel_db();
    let writer = crate::model::repository::sqlite::open_with_memory_limits(&path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    super::tests::insert_span(&writer, "span1", 1000, 1, 1);
    writer
        .execute(
            "INSERT INTO span_attributes VALUES ('span1', 'payload', 'before')",
            [],
        )
        .unwrap();
    let mut reader = open_sqlite_readonly(&path).unwrap();
    let tx = reader.transaction().unwrap();
    assert_eq!(
        super::budget::plan_count(&tx, &TimestampCursorWatermark::initial(), 100).unwrap(),
        1
    );
    writer
        .execute(
            "UPDATE span_attributes SET value = ?1",
            ["x".repeat(Config::get().max_transcript_line_bytes())],
        )
        .unwrap();
    let attributes = read_attributes_for_spans(&tx, &["span1"]).unwrap();
    assert_eq!(attributes["span1"]["payload"], "before");
    drop(tx);
    assert!(matches!(
        read_otel_spans_incremental(&path, Box::new(TimestampCursorWatermark::initial()), 100),
        Err(StreamError::Transient { .. })
    ));
}
