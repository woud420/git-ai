use super::map_sqlite_error;
use crate::model::stream_types::StreamError;
use rusqlite::Connection;
use serde_json::json;
use std::collections::HashMap;

pub(super) fn query_for_fields(fields: &str) -> String {
    format!(
        "SELECT {fields} FROM spans
        WHERE (end_time_ms > ?1 OR (?2 != '' AND end_time_ms = ?1 AND span_id > ?2))
          AND ((chat_session_id IS NOT NULL AND chat_session_id != '')
            OR (conversation_id IS NOT NULL AND conversation_id != ''))
        ORDER BY end_time_ms ASC, span_id ASC LIMIT ?3"
    )
}

pub(super) struct SpanRow {
    pub(super) span_id: String,
    pub(super) trace_id: String,
    pub(super) parent_span_id: Option<String>,
    pub(super) name: String,
    pub(super) start_time_ms: f64,
    pub(super) end_time_ms: f64,
    pub(super) status_code: i32,
    pub(super) status_message: Option<String>,
    pub(super) operation_name: Option<String>,
    pub(super) provider_name: Option<String>,
    pub(super) agent_name: Option<String>,
    pub(super) conversation_id: Option<String>,
    pub(super) request_model: Option<String>,
    pub(super) response_model: Option<String>,
    pub(super) input_tokens: Option<i64>,
    pub(super) output_tokens: Option<i64>,
    pub(super) cached_tokens: Option<i64>,
    pub(super) reasoning_tokens: Option<i64>,
    pub(super) tool_name: Option<String>,
    pub(super) tool_call_id: Option<String>,
    pub(super) tool_type: Option<String>,
    pub(super) chat_session_id: Option<String>,
    pub(super) turn_index: Option<i64>,
    pub(super) ttft_ms: Option<f64>,
}

pub(super) fn read_spans_after(
    conn: &Connection,
    after_ms: f64,
    after_id: &str,
    limit: usize,
) -> Result<Vec<SpanRow>, StreamError> {
    let sql = query_for_fields(
        "span_id, trace_id, parent_span_id, name,
         start_time_ms, end_time_ms,
         status_code, status_message, operation_name, provider_name, agent_name,
         conversation_id, request_model, response_model, input_tokens, output_tokens,
         cached_tokens, reasoning_tokens, tool_name, tool_call_id, tool_type,
         chat_session_id, turn_index, ttft_ms",
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| map_sqlite_error(e, "Failed to prepare spans query"))?;

    let rows = stmt
        .query_map(rusqlite::params![after_ms, after_id, limit], |row| {
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

pub(super) fn read_attributes_for_spans(
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

pub(super) fn read_events_for_spans(
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
