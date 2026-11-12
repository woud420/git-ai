use crate::model::stream_types::StreamError;
use crate::model::stream_watermark::TimestampCursorWatermark;
use crate::operations::streams::sqlite_budget::SqliteReadBudget;
use rusqlite::{Connection, params};

pub(super) fn plan_ids(
    conn: &Connection,
    session_id: &str,
    cursor: &TimestampCursorWatermark,
    row_hint: usize,
    v2: bool,
    has_parts: bool,
) -> Result<Vec<String>, StreamError> {
    let table = if v2 { "session_message" } else { "message" };
    let part = if v2 { "session_part" } else { "part" };
    let part_identity = if v2 {
        "CAST(p.ordinal AS TEXT)"
    } else {
        "p.id"
    };
    let extra_part_bytes = if v2 {
        "+ COALESCE(LENGTH(CAST(p.type AS BLOB)), 0) + COALESCE(LENGTH(CAST(p.call_id AS BLOB)), 0)"
    } else {
        ""
    };
    let extra_message_bytes = if v2 {
        "+ COALESCE(LENGTH(CAST(m.type AS BLOB)), 0) + COALESCE(LENGTH(CAST(m.seq AS BLOB)), 0)"
    } else {
        ""
    };
    let part_bytes = if has_parts {
        format!(
            "+ COALESCE((SELECT SUM(COALESCE(LENGTH(CAST({part_identity} AS BLOB)), 0) + COALESCE(LENGTH(CAST(p.message_id AS BLOB)), 0) + COALESCE(LENGTH(CAST(p.session_id AS BLOB)), 0) + COALESCE(LENGTH(CAST(p.data AS BLOB)), 0) {extra_part_bytes}) FROM {part} p WHERE p.message_id = m.id), 0)"
        )
    } else {
        String::new()
    };
    let sql = format!(
        "WITH candidates AS (SELECT id, time_updated FROM {table} WHERE session_id = ?1 AND (time_updated > ?2 OR (time_updated = ?2 AND id > ?3)) ORDER BY time_updated, id LIMIT ?4) SELECT m.id, COALESCE(LENGTH(CAST(m.id AS BLOB)), 0) + COALESCE(LENGTH(CAST(m.session_id AS BLOB)), 0) + COALESCE(LENGTH(CAST(m.data AS BLOB)), 0) {extra_message_bytes} {part_bytes} FROM candidates c JOIN {table} m ON m.id = c.id ORDER BY c.time_updated, c.id"
    );
    let mut stmt = conn.prepare(&sql).map_err(query_error)?;
    let mut rows = stmt
        .query(params![
            session_id,
            cursor.timestamp_millis as i64,
            cursor.last_id,
            row_hint
        ])
        .map_err(query_error)?;
    let mut budget = SqliteReadBudget::configured();
    let mut ids = Vec::new();
    while let Some(row) = rows.next().map_err(query_error)? {
        let bytes: u64 = row.get(1).map_err(query_error)?;
        if !budget.admit(bytes, bytes)? {
            break;
        }
        ids.push(row.get(0).map_err(query_error)?);
    }
    Ok(ids)
}
fn query_error(error: rusqlite::Error) -> StreamError {
    StreamError::Fatal {
        message: format!("Failed to plan OpenCode transcript bytes: {error}"),
    }
}
