use super::{map_sqlite_error, rows::query_for_fields};
use crate::model::stream_types::StreamError;
use crate::model::stream_watermark::TimestampCursorWatermark;
use crate::operations::streams::sqlite_budget::SqliteReadBudget;
use rusqlite::{Connection, params};

pub(super) fn plan_count(
    conn: &Connection,
    cursor: &TimestampCursorWatermark,
    row_limit: usize,
) -> Result<usize, StreamError> {
    let span_bytes = text_bytes(
        "spans",
        &[
            "span_id",
            "trace_id",
            "parent_span_id",
            "name",
            "status_message",
            "operation_name",
            "provider_name",
            "agent_name",
            "conversation_id",
            "request_model",
            "response_model",
            "tool_name",
            "tool_call_id",
            "tool_type",
            "chat_session_id",
        ],
    );
    let attribute_bytes = text_bytes("a", &["span_id", "key", "value"]);
    let event_bytes = text_bytes("e", &["span_id", "name", "attributes"]);
    let candidates = query_for_fields(&format!("span_id, end_time_ms, {span_bytes} AS bytes"));
    let sql = format!(
        "WITH candidates AS ({candidates}),
         attribute_bytes AS (
             SELECT a.span_id, SUM({attribute_bytes}) AS bytes FROM span_attributes a
             WHERE a.span_id IN (SELECT span_id FROM candidates) GROUP BY a.span_id
         ), event_bytes AS (
             SELECT e.span_id, SUM({event_bytes}) AS bytes FROM span_events e
             WHERE e.span_id IN (SELECT span_id FROM candidates) GROUP BY e.span_id
         )
         SELECT s.bytes + COALESCE(a.bytes, 0) + COALESCE(e.bytes, 0)
         FROM candidates s LEFT JOIN attribute_bytes a ON a.span_id = s.span_id
         LEFT JOIN event_bytes e ON e.span_id = s.span_id
         ORDER BY s.end_time_ms, s.span_id"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| map_sqlite_error(error, "Failed to prepare OTEL byte plan"))?;
    let mut rows = stmt
        .query(params![cursor.timestamp_millis, cursor.last_id, row_limit])
        .map_err(|error| map_sqlite_error(error, "Failed to query OTEL byte plan"))?;
    let mut budget = SqliteReadBudget::configured();
    let mut count = 0;
    while let Some(row) = rows
        .next()
        .map_err(|error| map_sqlite_error(error, "Failed to read OTEL byte plan"))?
    {
        let bytes = row
            .get(0)
            .map_err(|error| map_sqlite_error(error, "Invalid OTEL byte count"))?;
        if !budget.admit(bytes, bytes)? {
            break;
        }
        count += 1;
    }
    Ok(count)
}

fn text_bytes(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("COALESCE(LENGTH(CAST({table}.{column} AS BLOB)), 0)"))
        .collect::<Vec<_>>()
        .join(" + ")
}
