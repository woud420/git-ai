use super::open_sqlite_readonly;
use crate::model::stream_types::{StreamBatch, StreamError};
use crate::model::stream_watermark::TimestampWatermark;
use chrono::DateTime;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

pub(super) fn read_batch(
    path: &Path,
    watermark: &TimestampWatermark,
    session_id: &str,
    row_hint: usize,
) -> Result<StreamBatch, StreamError> {
    let mut conn = open_sqlite_readonly(path)?;
    // Planning and materialization must observe the same rows, even if the
    // agent appends or rewrites a large part while this poll runs.
    let tx = conn.transaction().map_err(|error| StreamError::Fatal {
        message: format!("Failed to start transcript read snapshot: {error}"),
    })?;
    let watermark_millis = watermark.0.timestamp_millis();
    let through_updated = super::budget::plan_through(&tx, session_id, watermark_millis, row_hint)?
        .unwrap_or(watermark_millis);
    let messages =
        read_session_messages_through(&tx, session_id, watermark_millis, through_updated)?;

    if messages.is_empty() {
        return Ok(StreamBatch {
            events: Vec::new(),
            new_watermark: Box::new(TimestampWatermark::new(watermark.0)),
        });
    }

    // Read only parts for the selected timestamp range in the same snapshot.
    let mut parts_by_message =
        read_parts_for_messages_through(&tx, session_id, watermark_millis, through_updated)?;

    let mut max_updated: i64 = watermark_millis;
    let mut events = Vec::with_capacity(messages.len());

    for (msg_id, time_updated, msg_data) in messages {
        if time_updated > max_updated {
            max_updated = time_updated;
        }

        // Use .remove() to move parts out of the HashMap instead of cloning via .get()
        let mut map = serde_json::Map::with_capacity(2);
        map.insert("message".into(), msg_data);
        if let Some(parts) = parts_by_message.remove(&msg_id) {
            map.insert("parts".into(), serde_json::Value::Array(parts));
        }

        events.push(serde_json::Value::Object(map));
    }

    let new_watermark_ts = DateTime::from_timestamp_millis(max_updated).unwrap_or(watermark.0);
    let new_watermark = Box::new(TimestampWatermark::new(new_watermark_ts));

    Ok(StreamBatch {
        events,
        new_watermark,
    })
}

/// Read messages from the database, returning each row as a complete JSON object
/// containing all columns (id, session_id, time_created, time_updated, data).
fn read_session_messages_through(
    conn: &Connection,
    session_id: &str,
    after_updated: i64,
    through_updated: i64,
) -> Result<Vec<(String, i64, serde_json::Value)>, StreamError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, time_created, time_updated, data FROM message \
             WHERE session_id = ? AND time_updated > ? AND time_updated <= ? \
             ORDER BY time_updated ASC, id ASC",
        )
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to prepare message query: {}", e),
        })?;

    let rows = stmt
        .query_map(
            rusqlite::params![session_id, after_updated, through_updated],
            |row| {
                let id: String = row.get(0)?;
                let row_session_id: String = row.get(1)?;
                let time_created: i64 = row.get(2)?;
                let time_updated: i64 = row.get(3)?;
                let data: String = row.get(4)?;
                Ok((id, row_session_id, time_created, time_updated, data))
            },
        )
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to query messages: {}", e),
        })?;

    let mut messages = Vec::new();
    for row in rows {
        let (id, row_session_id, time_created, time_updated, data) =
            row.map_err(|e| StreamError::Fatal {
                message: format!("Failed to read message row: {}", e),
            })?;

        let parsed_data: serde_json::Value =
            serde_json::from_str(&data).map_err(|e| StreamError::Parse {
                line: 0,
                message: format!("Failed to parse message data for id {}: {}", id, e),
            })?;

        // Build directly via Map to move parsed_data instead of cloning (json! macro clones)
        let mut map = serde_json::Map::with_capacity(5);
        map.insert("id".into(), serde_json::Value::String(id.clone()));
        map.insert(
            "session_id".into(),
            serde_json::Value::String(row_session_id),
        );
        map.insert(
            "time_created".into(),
            serde_json::Value::Number(time_created.into()),
        );
        map.insert(
            "time_updated".into(),
            serde_json::Value::Number(time_updated.into()),
        );
        map.insert("data".into(), parsed_data);

        messages.push((id, time_updated, serde_json::Value::Object(map)));
    }

    Ok(messages)
}

/// Read parts for the matched messages only, using an IN-subquery to avoid loading
/// all parts for the entire session. Returns each row as a complete JSON object
/// containing all columns (id, message_id, session_id, time_created, time_updated, data),
/// grouped by message_id.
pub(super) fn read_parts_for_messages_through(
    conn: &Connection,
    session_id: &str,
    after_updated: i64,
    through_updated: i64,
) -> Result<HashMap<String, Vec<serde_json::Value>>, StreamError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, message_id, session_id, time_created, time_updated, data FROM part \
             WHERE message_id IN ( \
                 SELECT id FROM message WHERE session_id = ? AND time_updated > ? AND time_updated <= ? ORDER BY time_updated ASC, id ASC \
             ) \
             ORDER BY message_id ASC, time_updated ASC, id ASC",
        )
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to prepare part query: {}", e),
        })?;

    let rows = stmt
        .query_map(
            rusqlite::params![session_id, after_updated, through_updated],
            |row| {
                let id: String = row.get(0)?;
                let message_id: String = row.get(1)?;
                let row_session_id: String = row.get(2)?;
                let time_created: i64 = row.get(3)?;
                let time_updated: i64 = row.get(4)?;
                let data: String = row.get(5)?;
                Ok((
                    id,
                    message_id,
                    row_session_id,
                    time_created,
                    time_updated,
                    data,
                ))
            },
        )
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to query parts: {}", e),
        })?;

    let mut parts_by_message: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for row in rows {
        let (id, message_id, row_session_id, time_created, time_updated, data) =
            row.map_err(|e| StreamError::Fatal {
                message: format!("Failed to read part row: {}", e),
            })?;

        if let Ok(parsed_data) = serde_json::from_str::<serde_json::Value>(&data) {
            let mut map = serde_json::Map::with_capacity(6);
            map.insert("id".into(), serde_json::Value::String(id));
            map.insert(
                "message_id".into(),
                serde_json::Value::String(message_id.clone()),
            );
            map.insert(
                "session_id".into(),
                serde_json::Value::String(row_session_id),
            );
            map.insert(
                "time_created".into(),
                serde_json::Value::Number(time_created.into()),
            );
            map.insert(
                "time_updated".into(),
                serde_json::Value::Number(time_updated.into()),
            );
            map.insert("data".into(), parsed_data);
            parts_by_message
                .entry(message_id)
                .or_default()
                .push(serde_json::Value::Object(map));
        }
    }

    Ok(parts_by_message)
}
