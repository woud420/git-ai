use crate::model::stream_types::{StreamBatch, StreamError};
use crate::model::stream_watermark::{TimestampCursorWatermark, WatermarkStrategy};
use crate::operations::streams::agents::opencode::open_sqlite_readonly;
use rusqlite::Connection;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

fn map_sqlite_error(e: rusqlite::Error, context: &str) -> StreamError {
    if let rusqlite::Error::SqliteFailure(ref err, _) = e
        && (err.code == rusqlite::ffi::ErrorCode::DatabaseBusy
            || err.code == rusqlite::ffi::ErrorCode::DatabaseLocked)
    {
        return StreamError::Transient {
            message: format!("{}: {}", context, e),
            retry_after: Duration::from_secs(2),
        };
    }
    StreamError::Fatal {
        message: format!("{}: {}", context, e),
    }
}

/// Read OTEL spans incrementally from a Copilot traces SQLite DB.
///
/// Uses keyset pagination on `(end_time_ms, span_id)` to prevent data loss
/// when multiple spans share the same `end_time_ms` at a batch boundary.
pub fn read_otel_spans_incremental(
    path: &Path,
    watermark: Box<dyn WatermarkStrategy>,
    batch_size: usize,
) -> Result<StreamBatch, StreamError> {
    let cursor = watermark
        .as_any()
        .downcast_ref::<TimestampCursorWatermark>()
        .ok_or_else(|| StreamError::Fatal {
            message: "OTEL stream requires TimestampCursorWatermark".to_string(),
        })?;

    let conn = open_sqlite_readonly(path)?;

    let spans = read_spans_after(&conn, cursor.timestamp_millis, &cursor.last_id, batch_size)?;
    if spans.is_empty() {
        return Ok(StreamBatch {
            events: vec![],
            new_watermark: Box::new(cursor.clone()),
        });
    }

    let span_ids: Vec<&str> = spans.iter().map(|s| s.span_id.as_str()).collect();
    let attributes = read_attributes_for_spans(&conn, &span_ids)?;
    let events = read_events_for_spans(&conn, &span_ids)?;

    let last_span = spans.last().unwrap();
    let new_watermark =
        TimestampCursorWatermark::new(last_span.end_time_ms, last_span.span_id.clone());

    let json_events: Vec<serde_json::Value> = spans
        .into_iter()
        .map(|span| {
            let span_attrs = attributes.get(&span.span_id).cloned().unwrap_or_default();
            let span_events = events.get(&span.span_id).cloned().unwrap_or_default();
            build_span_event_json(span, span_attrs, span_events)
        })
        .collect();

    Ok(StreamBatch {
        events: json_events,
        new_watermark: Box::new(new_watermark),
    })
}

struct SpanRow {
    span_id: String,
    trace_id: String,
    parent_span_id: Option<String>,
    name: String,
    start_time_ms: f64,
    end_time_ms: f64,
    status_code: i32,
    status_message: Option<String>,
    operation_name: Option<String>,
    provider_name: Option<String>,
    agent_name: Option<String>,
    conversation_id: Option<String>,
    request_model: Option<String>,
    response_model: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    tool_name: Option<String>,
    tool_call_id: Option<String>,
    tool_type: Option<String>,
    chat_session_id: Option<String>,
    turn_index: Option<i64>,
    ttft_ms: Option<f64>,
}

fn read_spans_after(
    conn: &Connection,
    after_ms: f64,
    after_id: &str,
    limit: usize,
) -> Result<Vec<SpanRow>, StreamError> {
    // Keyset pagination: skip spans at or before the cursor.
    // If after_id is empty (initial state), use simple `>` on timestamp.
    // Otherwise use compound `(ts > ?) OR (ts = ? AND id > ?)` to handle ties.
    // Only read spans that have at least one session identifier (chat_session_id
    // or conversation_id). Spans without either cannot be linked to a session.
    let session_filter = "(chat_session_id IS NOT NULL AND chat_session_id != '') \
                          OR (conversation_id IS NOT NULL AND conversation_id != '')";

    let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if after_id.is_empty() {
        (
            format!(
                "SELECT span_id, trace_id, parent_span_id, name, \
                 start_time_ms, end_time_ms, \
                 status_code, status_message, operation_name, provider_name, agent_name, \
                 conversation_id, request_model, response_model, input_tokens, output_tokens, \
                 cached_tokens, reasoning_tokens, tool_name, tool_call_id, tool_type, \
                 chat_session_id, turn_index, ttft_ms \
                 FROM spans WHERE end_time_ms > ?1 AND ({}) \
                 ORDER BY end_time_ms ASC, span_id ASC LIMIT ?2",
                session_filter
            ),
            vec![
                Box::new(after_ms) as Box<dyn rusqlite::types::ToSql>,
                Box::new(limit as i64),
            ],
        )
    } else {
        (
            format!(
                "SELECT span_id, trace_id, parent_span_id, name, \
                 start_time_ms, end_time_ms, \
                 status_code, status_message, operation_name, provider_name, agent_name, \
                 conversation_id, request_model, response_model, input_tokens, output_tokens, \
                 cached_tokens, reasoning_tokens, tool_name, tool_call_id, tool_type, \
                 chat_session_id, turn_index, ttft_ms \
                 FROM spans WHERE ((end_time_ms > ?1) OR (end_time_ms = ?2 AND span_id > ?3)) \
                 AND ({}) \
                 ORDER BY end_time_ms ASC, span_id ASC LIMIT ?4",
                session_filter
            ),
            vec![
                Box::new(after_ms) as Box<dyn rusqlite::types::ToSql>,
                Box::new(after_ms),
                Box::new(after_id.to_string()),
                Box::new(limit as i64),
            ],
        )
    };

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| map_sqlite_error(e, "Failed to prepare spans query"))?;

    let rows = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), |row| {
            Ok(SpanRow {
                span_id: row.get(0)?,
                trace_id: row.get(1)?,
                parent_span_id: row.get(2)?,
                name: row.get(3)?,
                start_time_ms: row.get(4)?,
                end_time_ms: row.get(5)?,
                status_code: row.get(6)?,
                status_message: row.get(7)?,
                operation_name: row.get(8)?,
                provider_name: row.get(9)?,
                agent_name: row.get(10)?,
                conversation_id: row.get(11)?,
                request_model: row.get(12)?,
                response_model: row.get(13)?,
                input_tokens: row.get(14)?,
                output_tokens: row.get(15)?,
                cached_tokens: row.get(16)?,
                reasoning_tokens: row.get(17)?,
                tool_name: row.get(18)?,
                tool_call_id: row.get(19)?,
                tool_type: row.get(20)?,
                chat_session_id: row.get(21)?,
                turn_index: row.get(22)?,
                ttft_ms: row.get(23)?,
            })
        })
        .map_err(|e| map_sqlite_error(e, "Failed to query spans"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| map_sqlite_error(e, "Failed to read span row"))
}

fn read_attributes_for_spans(
    conn: &Connection,
    span_ids: &[&str],
) -> Result<HashMap<String, HashMap<String, String>>, StreamError> {
    if span_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders: String = span_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT span_id, key, value FROM span_attributes WHERE span_id IN ({})",
        placeholders
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| map_sqlite_error(e, "Failed to prepare attributes query"))?;

    let mut result: HashMap<String, HashMap<String, String>> = HashMap::new();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(span_ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| map_sqlite_error(e, "Failed to query attributes"))?;

    for row in rows {
        let (span_id, key, value) =
            row.map_err(|e| map_sqlite_error(e, "Failed to read attribute row"))?;
        if let Some(v) = value {
            result.entry(span_id).or_default().insert(key, v);
        }
    }
    Ok(result)
}

fn read_events_for_spans(
    conn: &Connection,
    span_ids: &[&str],
) -> Result<HashMap<String, Vec<serde_json::Value>>, StreamError> {
    if span_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders: String = span_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT span_id, name, CAST(timestamp_ms AS INTEGER), attributes FROM span_events \
         WHERE span_id IN ({}) ORDER BY timestamp_ms ASC",
        placeholders
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| map_sqlite_error(e, "Failed to prepare events query"))?;

    let mut result: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(span_ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| map_sqlite_error(e, "Failed to query events"))?;

    for row in rows {
        let (span_id, name, timestamp_ms, attributes_json) =
            row.map_err(|e| map_sqlite_error(e, "Failed to read event row"))?;
        let attrs: serde_json::Value = attributes_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::Value::Null);
        result.entry(span_id).or_default().push(json!({
            "name": name,
            "timestamp_ms": timestamp_ms,
            "attributes": attrs,
        }));
    }
    Ok(result)
}

fn build_span_event_json(
    span: SpanRow,
    attributes: HashMap<String, String>,
    events: Vec<serde_json::Value>,
) -> serde_json::Value {
    json!({
        "span": {
            "span_id": span.span_id,
            "trace_id": span.trace_id,
            "parent_span_id": span.parent_span_id,
            "name": span.name,
            "start_time_ms": span.start_time_ms as i64,
            "end_time_ms": span.end_time_ms as i64,
            "status_code": span.status_code,
            "status_message": span.status_message,
            "operation_name": span.operation_name,
            "provider_name": span.provider_name,
            "agent_name": span.agent_name,
            "conversation_id": span.conversation_id,
            "request_model": span.request_model,
            "response_model": span.response_model,
            "input_tokens": span.input_tokens,
            "output_tokens": span.output_tokens,
            "cached_tokens": span.cached_tokens,
            "reasoning_tokens": span.reasoning_tokens,
            "tool_name": span.tool_name,
            "tool_call_id": span.tool_call_id,
            "tool_type": span.tool_type,
            "chat_session_id": span.chat_session_id,
            "turn_index": span.turn_index,
            "ttft_ms": span.ttft_ms,
        },
        "attributes": attributes,
        "events": events,
    })
}

/// Extract per-event IDs from an OTEL span event JSON.
/// Returns (event_id=span_id, parent_event_id=parent_span_id, tool_use_id=tool_call_id).
pub fn extract_otel_event_ids(
    event: &serde_json::Value,
) -> (Option<String>, Option<String>, Option<String>) {
    let span = event.get("span");
    let event_id = span
        .and_then(|s| s.get("span_id"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let parent_event_id = span
        .and_then(|s| s.get("parent_span_id"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let tool_use_id = span
        .and_then(|s| s.get("tool_call_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    (event_id, parent_event_id, tool_use_id)
}

/// Extract timestamp (as Unix seconds u32) from an OTEL span event JSON.
pub fn extract_otel_event_timestamp(event: &serde_json::Value) -> Option<u32> {
    event
        .get("span")
        .and_then(|s| s.get("start_time_ms"))
        .and_then(|v| v.as_i64())
        .map(|ms| (ms / 1000) as u32)
}

#[path = "copilot_otel_tests.rs"]
#[cfg(test)]
mod tests;

#[path = "copilot_otel_identity_tests.rs"]
#[cfg(test)]
mod identity_tests;
