use super::{open_sqlite_readonly, schema};
use crate::model::stream_types::{StreamBatch, StreamError};
use crate::model::stream_watermark::TimestampCursorWatermark;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

pub(super) fn read_batch(
    path: &Path,
    cursor: &TimestampCursorWatermark,
    session_id: &str,
    row_hint: usize,
) -> Result<StreamBatch, StreamError> {
    let mut conn = open_sqlite_readonly(path)?;
    let tx = conn.transaction().map_err(|error| StreamError::Fatal {
        message: format!("Failed to start transcript read snapshot: {error}"),
    })?;
    let v2 = schema::is_v2_session(&tx, session_id);
    let has_parts = !v2 || schema::has_table(&tx, "session_part")?;
    let ids = super::budget::plan_ids(&tx, session_id, cursor, row_hint, v2, has_parts)?;
    let ids_json = serde_json::to_string(&ids).map_err(|e| StreamError::Fatal {
        message: format!("Failed to encode selected message IDs: {e}"),
    })?;
    let messages = read_messages(&tx, &ids_json, v2)?;
    let mut parts = if has_parts {
        read_parts_for_messages(&tx, &ids_json, v2)?
    } else {
        HashMap::new()
    };
    let mut next = cursor.clone();
    if let Some((id, updated, _)) = messages.last() {
        next = TimestampCursorWatermark::new(*updated as f64, id.clone());
    }
    let events = messages
        .into_iter()
        .map(|(id, _, message)| {
            let mut map = serde_json::Map::new();
            map.insert("message".into(), message);
            if let Some(parts) = parts.remove(&id) {
                map.insert("parts".into(), serde_json::Value::Array(parts));
            }
            serde_json::Value::Object(map)
        })
        .collect();
    Ok(StreamBatch {
        events,
        new_watermark: Box::new(next),
    })
}

fn read_messages(
    conn: &Connection,
    ids_json: &str,
    v2: bool,
) -> Result<Vec<(String, i64, serde_json::Value)>, StreamError> {
    let table = if v2 { "session_message" } else { "message" };
    let extra_columns = if v2 { ", type, seq" } else { "" };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, session_id, time_created, time_updated, data{extra_columns} FROM {table} \
             WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY time_updated ASC, id ASC"
        ))
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to prepare message query: {}", e),
        })?;

    let rows = stmt
        .query_map([ids_json], |row| {
            let id: String = row.get(0)?;
            let row_session_id: String = row.get(1)?;
            let time_created: i64 = row.get(2)?;
            let time_updated: i64 = row.get(3)?;
            let data: String = row.get(4)?;
            let v2_fields = if v2 {
                Some((row.get::<_, String>(5)?, row.get::<_, i64>(6)?))
            } else {
                None
            };
            Ok((
                id,
                row_session_id,
                time_created,
                time_updated,
                data,
                v2_fields,
            ))
        })
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to query messages: {}", e),
        })?;

    let mut messages = Vec::new();
    for row in rows {
        let (id, row_session_id, time_created, time_updated, data, v2_fields) =
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
        if let Some((message_type, seq)) = v2_fields {
            map.insert("type".into(), serde_json::Value::String(message_type));
            map.insert("seq".into(), serde_json::Value::Number(seq.into()));
        }

        messages.push((id, time_updated, serde_json::Value::Object(map)));
    }

    Ok(messages)
}

/// Fetch parts by the selected IDs without repeating the session scan/sort.
/// Returns native V1 or V2 rows grouped by
/// message_id, preserving V2's ordinal and call_id instead of inventing a part ID.
pub(super) fn read_parts_for_messages(
    conn: &Connection,
    ids_json: &str,
    v2: bool,
) -> Result<HashMap<String, Vec<serde_json::Value>>, StreamError> {
    let (table, identity, extra_columns, order) = if v2 {
        ("session_part", "ordinal", ", type, call_id", "ordinal ASC")
    } else {
        ("part", "id", "", "time_updated ASC, id ASC")
    };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {identity}, message_id, session_id, time_created, time_updated, data{extra_columns} FROM {table} \
             WHERE message_id IN (SELECT value FROM json_each(?1)) \
             ORDER BY message_id ASC, {order}",
        ))
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to prepare part query: {}", e),
        })?;

    let rows = stmt
        .query_map([ids_json], |row| {
            let id = if v2 {
                serde_json::Value::Number(row.get::<_, i64>(0)?.into())
            } else {
                serde_json::Value::String(row.get(0)?)
            };
            let message_id: String = row.get(1)?;
            let row_session_id: String = row.get(2)?;
            let time_created: i64 = row.get(3)?;
            let time_updated: i64 = row.get(4)?;
            let data: String = row.get(5)?;
            let v2_fields = if v2 {
                Some((row.get::<_, String>(6)?, row.get::<_, Option<String>>(7)?))
            } else {
                None
            };
            Ok((
                id,
                message_id,
                row_session_id,
                time_created,
                time_updated,
                data,
                v2_fields,
            ))
        })
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to query parts: {}", e),
        })?;

    let mut parts_by_message: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for row in rows {
        let (id, message_id, row_session_id, time_created, time_updated, data, v2_fields) = row
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to read part row: {}", e),
            })?;

        if let Ok(parsed_data) = serde_json::from_str::<serde_json::Value>(&data) {
            let mut map = serde_json::Map::with_capacity(6);
            map.insert(identity.into(), id);
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
            if let Some((part_type, call_id)) = v2_fields {
                map.insert("type".into(), serde_json::Value::String(part_type));
                map.insert(
                    "call_id".into(),
                    call_id
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                );
            }
            parts_by_message
                .entry(message_id)
                .or_default()
                .push(serde_json::Value::Object(map));
        }
    }

    Ok(parts_by_message)
}
