//! OpenCode agent implementation (SQLite-only).

pub use super::opencode_checkpoint::open_sqlite_readonly;
use crate::model::stream_types::{StreamBatch, StreamError};
use crate::model::stream_watermark::{TimestampWatermark, WatermarkStrategy};
use crate::operations::streams::agent::{Agent, StreamDescriptor};
use crate::operations::streams::sweep::{DiscoveredSession, StreamFormat, SweepStrategy};
use crate::operations::streams::timestamp::event_timestamp_or_file_time;
use chrono::DateTime;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// OpenCode agent that reads from an OpenCode SQLite database.
pub struct OpenCodeAgent {
    batch_size: usize,
}

impl OpenCodeAgent {
    pub fn new() -> Self {
        Self { batch_size: 1000 }
    }

    #[cfg(test)]
    pub fn with_batch_size(batch_size: usize) -> Self {
        Self { batch_size }
    }
}

/// Read messages from the database, returning each row as a complete JSON object
/// containing all columns (id, session_id, time_created, time_updated, data).
fn read_session_messages_raw_with_limit(
    conn: &Connection,
    session_id: &str,
    after_updated: i64,
    limit: usize,
) -> Result<Vec<(String, i64, serde_json::Value)>, StreamError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, time_created, time_updated, data FROM message \
             WHERE session_id = ? AND time_updated > ? \
             ORDER BY time_updated ASC, id ASC \
             LIMIT ?",
        )
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to prepare message query: {}", e),
        })?;

    let rows = stmt
        .query_map(rusqlite::params![session_id, after_updated, limit], |row| {
            let id: String = row.get(0)?;
            let row_session_id: String = row.get(1)?;
            let time_created: i64 = row.get(2)?;
            let time_updated: i64 = row.get(3)?;
            let data: String = row.get(4)?;
            Ok((id, row_session_id, time_created, time_updated, data))
        })
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
fn read_parts_for_messages_with_limit(
    conn: &Connection,
    session_id: &str,
    after_updated: i64,
    limit: usize,
) -> Result<HashMap<String, Vec<serde_json::Value>>, StreamError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, message_id, session_id, time_created, time_updated, data FROM part \
             WHERE message_id IN ( \
                 SELECT id FROM message WHERE session_id = ? AND time_updated > ? ORDER BY time_updated ASC, id ASC LIMIT ? \
             ) \
             ORDER BY message_id ASC, time_updated ASC, id ASC",
        )
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to prepare part query: {}", e),
        })?;

    let rows = stmt
        .query_map(rusqlite::params![session_id, after_updated, limit], |row| {
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
        })
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

impl Default for OpenCodeAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Agent for OpenCodeAgent {
    fn validate_checkpoint_stream(
        &self,
        source: &crate::model::checkpoint_request::StreamSource,
    ) -> Result<DiscoveredSession, StreamError> {
        super::opencode_checkpoint::validate_checkpoint_stream(source)
    }

    fn batch_size_hint(&self) -> usize {
        self.batch_size
    }

    fn sweep_strategy(&self) -> SweepStrategy {
        SweepStrategy::Periodic(Duration::from_secs(30 * 60))
    }

    fn discover_sessions(&self) -> Result<Vec<DiscoveredSession>, StreamError> {
        // Discovery comes from presets, not sweep.
        Ok(Vec::new())
    }

    fn session_id_for_read<'a>(&self, _: &'a str, external_session_id: &'a str) -> &'a str {
        external_session_id
    }

    fn read_incremental(
        &self,
        path: &Path,
        watermark: Box<dyn WatermarkStrategy>,
        session_id: &str,
    ) -> Result<StreamBatch, StreamError> {
        // Downcast to TimestampWatermark
        let ts_watermark = watermark
            .as_any()
            .downcast_ref::<TimestampWatermark>()
            .ok_or_else(|| StreamError::Fatal {
                message: format!(
                    "OpenCode reader requires TimestampWatermark, got incompatible type for session {}",
                    session_id
                ),
            })?;

        let watermark_millis = ts_watermark.0.timestamp_millis();

        // Open SQLite read-only
        let conn = open_sqlite_readonly(path)?;

        // LIMIT applied for memory safety. Uses strict > to avoid re-reading.
        // Note: messages sharing exact same millisecond as watermark boundary could
        // theoretically be skipped, but OpenCode writes are interactive (not concurrent)
        // so millisecond collisions are effectively impossible in practice.
        let messages = read_session_messages_raw_with_limit(
            &conn,
            session_id,
            watermark_millis,
            self.batch_size,
        )?;

        if messages.is_empty() {
            return Ok(StreamBatch {
                events: Vec::new(),
                new_watermark: Box::new(TimestampWatermark::new(ts_watermark.0)),
            });
        }

        // Read only parts for the matched messages (IN-subquery, single scan)
        let mut parts_by_message = read_parts_for_messages_with_limit(
            &conn,
            session_id,
            watermark_millis,
            self.batch_size,
        )?;

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

        let new_watermark_ts =
            DateTime::from_timestamp_millis(max_updated).unwrap_or(ts_watermark.0);
        let new_watermark = Box::new(TimestampWatermark::new(new_watermark_ts));

        Ok(StreamBatch {
            events,
            new_watermark,
        })
    }

    fn extract_event_ids(
        &self,
        event: &serde_json::Value,
    ) -> (Option<String>, Option<String>, Option<String>) {
        let message = event.get("message");

        let event_id = message
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let parent_event_id = message
            .and_then(|m| m.get("data"))
            .and_then(|d| d.get("parentID"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let tool_use_id = event
            .get("parts")
            .and_then(|p| p.as_array())
            .and_then(|arr| {
                arr.iter().find_map(|part| {
                    part.get("data")
                        .and_then(|d| d.get("callID"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
            });

        (event_id, parent_event_id, tool_use_id)
    }

    fn extract_event_timestamp(
        &self,
        event: &serde_json::Value,
        file_meta: &std::fs::Metadata,
        is_first_event: bool,
    ) -> u32 {
        event_timestamp_or_file_time(event, file_meta, is_first_event)
    }

    fn streams(&self) -> Vec<StreamDescriptor> {
        vec![StreamDescriptor::identity_transcript(
            StreamFormat::OpenCodeSqlite,
        )]
    }
}

#[path = "opencode_tests.rs"]
#[cfg(test)]
mod tests;
