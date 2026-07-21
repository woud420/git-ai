use crate::error::GitAiError;
use crate::metrics::attrs::attr_pos;
use crate::metrics::events::{checkpoint_pos, otel_trace_pos, session_event_pos};
use crate::metrics::types::MetricEventId;
use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value};

use super::MetricsDatabase;
use super::types::MetricEventMetadata;

pub(crate) const METADATA_BACKFILL_BATCH_SIZE: usize = 1000;

impl MetricsDatabase {
    /// Insert undelivered events as JSON strings.
    pub fn insert_events(&mut self, events: &[String]) -> Result<Vec<i64>, GitAiError> {
        self.insert_events_with_delivered_ts(events, None)
    }

    /// Insert events as JSON strings, optionally marking them delivered immediately.
    pub fn insert_events_with_delivered_ts(
        &mut self,
        events: &[String],
        delivered_ts: Option<u64>,
    ) -> Result<Vec<i64>, GitAiError> {
        if events.is_empty() {
            return Ok(Vec::new());
        }

        let tx = self.conn.transaction()?;
        let mut ids = Vec::with_capacity(events.len());

        {
            let mut stmt = tx.prepare_cached(
                r#"
                INSERT INTO metrics (
                    event_json,
                    delivered_ts,
                    event_ts,
                    event_kind,
                    trace_id,
                    session_id,
                    parent_session_id,
                    tool,
                    external_session_id,
                    external_parent_session_id,
                    external_event_id,
                    external_parent_event_id,
                    external_tool_use_id
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
            )?;

            for event_json in events {
                let metadata = extract_metric_event_metadata(event_json);
                let event_ts = metadata.as_ref().map(|m| i64::from(m.event_ts));
                let event_kind = metadata.as_ref().map(|m| i64::from(m.event_kind));
                let delivered_ts = delivered_ts.map(|ts| ts as i64);

                stmt.execute(params![
                    event_json,
                    delivered_ts,
                    event_ts,
                    event_kind,
                    metadata.as_ref().and_then(|m| m.trace_id.as_deref()),
                    metadata.as_ref().and_then(|m| m.session_id.as_deref()),
                    metadata
                        .as_ref()
                        .and_then(|m| m.parent_session_id.as_deref()),
                    metadata.as_ref().and_then(|m| m.tool.as_deref()),
                    metadata
                        .as_ref()
                        .and_then(|m| m.external_session_id.as_deref()),
                    metadata
                        .as_ref()
                        .and_then(|m| m.external_parent_session_id.as_deref()),
                    metadata
                        .as_ref()
                        .and_then(|m| m.external_event_id.as_deref()),
                    metadata
                        .as_ref()
                        .and_then(|m| m.external_parent_event_id.as_deref()),
                    metadata
                        .as_ref()
                        .and_then(|m| m.external_tool_use_id.as_deref()),
                ])?;
                ids.push(tx.last_insert_rowid());
            }
        }

        tx.commit()?;
        self.prune_old_metrics_if_due()?;
        Ok(ids)
    }

    /// Delete metric rows outside the local retention window.
    ///
    /// Valid rows are pruned by event timestamp, regardless of delivery state. Malformed
    /// rows cannot be aged by event timestamp, so delivered malformed rows fall back to
    /// `delivered_ts`.
    pub(super) fn prune_old_metrics_if_due(&mut self) -> Result<(), GitAiError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let last_prune: Option<i64> = self
            .conn
            .query_row(
                "SELECT value FROM schema_metadata WHERE key = 'metrics_last_prune_ts'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .and_then(|v: String| v.parse().ok());

        if let Some(last) = last_prune
            && now.saturating_sub(last as u64) < Self::METRICS_PRUNE_INTERVAL_SECS
        {
            return Ok(());
        }

        let cutoff = now.saturating_sub(Self::METRICS_RETENTION_SECS);
        let rows_to_prune = self.old_metric_row_ids(cutoff)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO schema_metadata (key, value) VALUES ('metrics_last_prune_ts', ?1)",
            params![now.to_string()],
        )?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM metrics WHERE id = ?1")?;
            for id in rows_to_prune {
                stmt.execute(params![id])?;
            }
        }
        tx.commit()?;

        Ok(())
    }

    fn old_metric_row_ids(&self, cutoff: u64) -> Result<Vec<i64>, GitAiError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_json, event_ts, delivered_ts FROM metrics ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?;

        let mut ids = Vec::new();
        for row in rows {
            let (id, event_json, event_ts, delivered_ts) = row?;
            if metric_row_is_older_than_cutoff(&event_json, event_ts, delivered_ts, cutoff) {
                ids.push(id);
            }
        }

        Ok(ids)
    }
}

pub(super) fn extract_metric_event_metadata(event_json: &str) -> Option<MetricEventMetadata> {
    let value: Value = serde_json::from_str(event_json).ok()?;
    let event_ts = extract_metric_event_ts_from_value(&value)?;
    let event_kind = value
        .get("e")
        .and_then(Value::as_u64)
        .filter(|kind| *kind <= u16::MAX as u64)? as u16;

    let attrs = value.get("a").and_then(Value::as_object);
    let values = value.get("v").and_then(Value::as_object);

    Some(MetricEventMetadata {
        event_ts,
        event_kind,
        trace_id: sparse_object_string(attrs, attr_pos::TRACE_ID),
        session_id: sparse_object_string(attrs, attr_pos::SESSION_ID),
        parent_session_id: sparse_object_string(attrs, attr_pos::PARENT_SESSION_ID),
        tool: sparse_object_string(attrs, attr_pos::TOOL),
        external_session_id: sparse_object_string(attrs, attr_pos::EXTERNAL_SESSION_ID),
        external_parent_session_id: sparse_object_string(
            attrs,
            attr_pos::EXTERNAL_PARENT_SESSION_ID,
        ),
        external_event_id: event_specific_external_event_id(event_kind, values),
        external_parent_event_id: event_specific_external_parent_event_id(event_kind, values),
        external_tool_use_id: event_specific_external_tool_use_id(event_kind, values),
    })
}

pub(super) fn sparse_object_string(
    object: Option<&Map<String, Value>>,
    pos: usize,
) -> Option<String> {
    object?
        .get(&pos.to_string())
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

pub(super) fn extract_metric_event_ts_from_value(value: &Value) -> Option<u32> {
    value
        .get("t")
        .and_then(Value::as_u64)
        .filter(|ts| *ts <= u32::MAX as u64)
        .map(|ts| ts as u32)
}

fn metric_row_is_older_than_cutoff(
    event_json: &str,
    event_ts: Option<i64>,
    delivered_ts: Option<i64>,
    cutoff: u64,
) -> bool {
    if let Some(ts) = event_ts
        && ts >= 0
    {
        return (ts as u64) < cutoff;
    }

    if let Some(ts) = extract_metric_event_ts(event_json) {
        return u64::from(ts) < cutoff;
    }

    delivered_ts.is_some_and(|ts| ts >= 0 && (ts as u64) < cutoff)
}

fn extract_metric_event_ts(event_json: &str) -> Option<u32> {
    let value: Value = serde_json::from_str(event_json).ok()?;
    extract_metric_event_ts_from_value(&value)
}

fn event_specific_external_event_id(
    event_kind: u16,
    values: Option<&Map<String, Value>>,
) -> Option<String> {
    if event_kind == MetricEventId::SessionEvent as u16 {
        return sparse_object_string(values, session_event_pos::EXTERNAL_EVENT_ID);
    }
    if event_kind == MetricEventId::OtelTrace as u16 {
        return sparse_object_string(values, otel_trace_pos::EXTERNAL_EVENT_ID);
    }
    None
}

fn event_specific_external_parent_event_id(
    event_kind: u16,
    values: Option<&Map<String, Value>>,
) -> Option<String> {
    if event_kind == MetricEventId::SessionEvent as u16 {
        return sparse_object_string(values, session_event_pos::EXTERNAL_PARENT_EVENT_ID);
    }
    if event_kind == MetricEventId::OtelTrace as u16 {
        return sparse_object_string(values, otel_trace_pos::EXTERNAL_PARENT_EVENT_ID);
    }
    None
}

fn event_specific_external_tool_use_id(
    event_kind: u16,
    values: Option<&Map<String, Value>>,
) -> Option<String> {
    if event_kind == MetricEventId::Checkpoint as u16 {
        return sparse_object_string(values, checkpoint_pos::TOOL_USE_ID);
    }
    if event_kind == MetricEventId::SessionEvent as u16 {
        return sparse_object_string(values, session_event_pos::EXTERNAL_TOOL_USE_ID);
    }
    if event_kind == MetricEventId::OtelTrace as u16 {
        return sparse_object_string(values, otel_trace_pos::EXTERNAL_TOOL_USE_ID);
    }
    None
}
