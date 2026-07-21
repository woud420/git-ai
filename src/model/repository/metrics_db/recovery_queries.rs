use crate::error::GitAiError;
use crate::metrics::attrs::attr_pos;
use crate::metrics::types::MetricEventId;
use rusqlite::{params, params_from_iter};
use serde_json::Value;

use super::MetricsDatabase;
use super::event_writes::sparse_object_string;
use super::types::SessionEventRecoveryCandidate;

pub(crate) const NS_PER_SECOND: u128 = 1_000_000_000;

impl MetricsDatabase {
    pub(crate) fn session_event_candidates_near_timestamps(
        &self,
        timestamps_ns: &[u128],
        window_ns: u128,
    ) -> Result<Vec<SessionEventRecoveryCandidate>, GitAiError> {
        if timestamps_ns.is_empty() {
            return Ok(Vec::new());
        }

        let Some((min_event_ts, max_event_ts)) =
            event_ts_bounds_for_ns_windows(timestamps_ns, window_ns)
        else {
            return Ok(Vec::new());
        };

        let mut stmt = self.conn.prepare(
            r#"
            SELECT
                id,
                event_json,
                event_ts,
                session_id,
                trace_id,
                tool,
                external_session_id,
                external_tool_use_id
            FROM metrics
            WHERE event_kind = ?1
              AND event_ts >= ?2
              AND event_ts <= ?3
              AND session_id IS NOT NULL
              AND session_id != ''
              AND tool IS NOT NULL
              AND tool != ''
              AND tool != 'mock_ai'
              AND external_session_id IS NOT NULL
              AND external_session_id != ''
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map(
            params![
                MetricEventId::SessionEvent as i64,
                min_event_ts as i64,
                max_event_ts as i64
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )?;

        let mut candidates = Vec::new();
        for row in rows {
            let (
                row_id,
                event_json,
                event_ts,
                session_id,
                trace_id,
                tool,
                external_session_id,
                external_tool_use_id,
            ) = row?;
            if event_ts < 0 || event_ts > u32::MAX as i64 {
                continue;
            }
            let event_ts = event_ts as u32;
            if min_distance_to_event_ts(timestamps_ns, event_ts)
                .is_none_or(|distance| distance > window_ns)
            {
                continue;
            }

            let (repo_url, model) = recovery_attrs_from_event_json(&event_json);
            candidates.push(SessionEventRecoveryCandidate {
                row_id,
                event_ts,
                session_id,
                trace_id,
                tool,
                model,
                external_session_id,
                external_tool_use_id,
                repo_url,
            });
        }

        Ok(candidates)
    }

    pub(crate) fn latest_session_event_candidates_for_tools(
        &self,
        tools: &[&str],
    ) -> Result<Vec<SessionEventRecoveryCandidate>, GitAiError> {
        if tools.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = std::iter::repeat_n("?", tools.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT
                id,
                event_json,
                event_ts,
                session_id,
                trace_id,
                tool,
                external_session_id,
                external_tool_use_id
            FROM metrics
            WHERE event_kind = ?1
              AND tool IN ({placeholders})
              AND event_ts IS NOT NULL
              AND session_id IS NOT NULL
              AND session_id != ''
              AND tool IS NOT NULL
              AND tool != ''
              AND tool != 'mock_ai'
              AND external_session_id IS NOT NULL
              AND external_session_id != ''
            ORDER BY event_ts DESC, id DESC
            LIMIT 100
            "#
        );

        let mut values = Vec::with_capacity(tools.len() + 1);
        values.push(rusqlite::types::Value::Integer(
            MetricEventId::SessionEvent as i64,
        ));
        values.extend(
            tools
                .iter()
                .map(|tool| rusqlite::types::Value::Text((*tool).to_string())),
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter()), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?;

        let mut candidates = Vec::new();
        for row in rows {
            let (
                row_id,
                event_json,
                event_ts,
                session_id,
                trace_id,
                tool,
                external_session_id,
                external_tool_use_id,
            ) = row?;
            if event_ts < 0 || event_ts > u32::MAX as i64 {
                continue;
            }

            let (repo_url, model) = recovery_attrs_from_event_json(&event_json);
            candidates.push(SessionEventRecoveryCandidate {
                row_id,
                event_ts: event_ts as u32,
                session_id,
                trace_id,
                tool,
                model,
                external_session_id,
                external_tool_use_id,
                repo_url,
            });
        }

        Ok(candidates)
    }
}

fn event_ts_bounds_for_ns_windows(timestamps_ns: &[u128], window_ns: u128) -> Option<(u32, u32)> {
    let mut min_ts: Option<u32> = None;
    let mut max_ts: Option<u32> = None;
    for timestamp_ns in timestamps_ns {
        let start = timestamp_ns.saturating_sub(window_ns) / NS_PER_SECOND;
        let end = timestamp_ns
            .saturating_add(window_ns)
            .min(u32::MAX as u128 * NS_PER_SECOND)
            / NS_PER_SECOND;
        let start = start.min(u32::MAX as u128) as u32;
        let end = end.min(u32::MAX as u128) as u32;
        min_ts = Some(min_ts.map_or(start, |current| current.min(start)));
        max_ts = Some(max_ts.map_or(end, |current| current.max(end)));
    }
    min_ts.zip(max_ts)
}

fn min_distance_to_event_ts(timestamps_ns: &[u128], event_ts: u32) -> Option<u128> {
    timestamps_ns
        .iter()
        .map(|timestamp_ns| distance_to_event_second(*timestamp_ns, event_ts))
        .min()
}

fn distance_to_event_second(timestamp_ns: u128, event_ts: u32) -> u128 {
    let start_ns = event_ts as u128 * NS_PER_SECOND;
    let end_ns = start_ns.saturating_add(NS_PER_SECOND - 1);
    if timestamp_ns < start_ns {
        start_ns - timestamp_ns
    } else {
        timestamp_ns.saturating_sub(end_ns)
    }
}

fn recovery_attrs_from_event_json(event_json: &str) -> (Option<String>, Option<String>) {
    let Ok(value) = serde_json::from_str::<Value>(event_json) else {
        return (None, None);
    };
    let attrs = value.get("a").and_then(Value::as_object);
    (
        sparse_object_string(attrs, attr_pos::REPO_URL),
        sparse_object_string(attrs, attr_pos::MODEL),
    )
}
