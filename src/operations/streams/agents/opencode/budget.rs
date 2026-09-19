use crate::model::stream_types::StreamError;
use crate::operations::streams::sqlite_budget::SqliteReadBudget;
use rusqlite::{Connection, params};

pub(super) fn plan_through(
    conn: &Connection,
    session_id: &str,
    after_updated: i64,
    row_hint: usize,
) -> Result<Option<i64>, StreamError> {
    if row_hint == 0 {
        return Ok(None);
    }
    // The cursor stores only a timestamp. Measure entire tied groups before
    // loading any TEXT, and count UTF-8 bytes rather than SQLite characters.
    let mut stmt = conn
        .prepare(
            "WITH candidate_times AS (
             SELECT DISTINCT time_updated FROM message
             WHERE session_id = ?1 AND time_updated > ?2
             ORDER BY time_updated LIMIT ?3
         ), candidate_messages AS (
             SELECT id, time_updated,
                 COALESCE(LENGTH(CAST(id AS BLOB)), 0) +
                 COALESCE(LENGTH(CAST(session_id AS BLOB)), 0) +
                 COALESCE(LENGTH(CAST(data AS BLOB)), 0) AS bytes
             FROM message
             WHERE session_id = ?1 AND time_updated IN (SELECT time_updated FROM candidate_times)
         ), part_bytes AS (
             SELECT message_id, SUM(
                 COALESCE(LENGTH(CAST(id AS BLOB)), 0) +
                 COALESCE(LENGTH(CAST(message_id AS BLOB)), 0) +
                 COALESCE(LENGTH(CAST(session_id AS BLOB)), 0) +
                 COALESCE(LENGTH(CAST(data AS BLOB)), 0)
             ) AS bytes FROM part
             WHERE message_id IN (SELECT id FROM candidate_messages)
             GROUP BY message_id
         )
         SELECT m.time_updated, COUNT(*),
             SUM(m.bytes + COALESCE(p.bytes, 0)), MAX(m.bytes + COALESCE(p.bytes, 0))
         FROM candidate_messages m LEFT JOIN part_bytes p ON p.message_id = m.id
         GROUP BY m.time_updated ORDER BY m.time_updated",
        )
        .map_err(query_error)?;
    let mut rows = stmt
        .query(params![session_id, after_updated, row_hint])
        .map_err(query_error)?;
    let mut budget = SqliteReadBudget::configured();
    let mut count = 0_u64;
    let mut through = None;
    while let Some(row) = rows.next().map_err(query_error)? {
        let bytes = row.get(2).map_err(query_error)?;
        let largest_event = row.get(3).map_err(query_error)?;
        if !budget.admit(bytes, largest_event)? {
            break;
        }
        through = Some(row.get(0).map_err(query_error)?);
        count = count.saturating_add(row.get(1).map_err(query_error)?);
        if count >= row_hint as u64 {
            break;
        }
    }
    Ok(through)
}

fn query_error(error: rusqlite::Error) -> StreamError {
    StreamError::Fatal {
        message: format!("Failed to plan OpenCode transcript bytes: {error}"),
    }
}
