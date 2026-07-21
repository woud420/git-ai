use crate::error::GitAiError;
use crate::metrics::attrs::attr_pos;
use crate::metrics::pos_encoded::sparse_get_string;
use rusqlite::{OptionalExtension, params};

use super::MetricsDatabase;
use super::schema::MAX_METRIC_UPLOAD_ATTEMPTS;
use super::types::{MetricHistoryRecord, MetricsStatus};
use super::upload_queue::current_unix_ts;

impl MetricsDatabase {
    /// Get count of pending metrics that are currently eligible for upload.
    pub fn count_retryable(&self) -> Result<usize, GitAiError> {
        let now = current_unix_ts();
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM metrics \
             WHERE delivered_ts IS NULL \
               AND processing_started_at IS NULL \
               AND next_retry_at <= ?1 \
               AND attempts < 6",
            params![now as i64],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Summarize local metrics delivery state for user-facing diagnostics.
    pub fn status(&self) -> Result<MetricsStatus, GitAiError> {
        let now = current_unix_ts();
        let (
            total,
            delivered,
            not_delivered,
            pending_retryable,
            waiting_retry,
            processing,
            stopped_after_errors,
            rows_with_errors,
        ): (i64, i64, i64, i64, i64, i64, i64, i64) = self.conn.query_row(
            r#"
            SELECT
                COUNT(*),
                COALESCE(SUM(CASE WHEN delivered_ts IS NOT NULL THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN delivered_ts IS NULL THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE
                    WHEN delivered_ts IS NULL
                     AND processing_started_at IS NULL
                     AND next_retry_at <= ?1
                     AND attempts < ?2 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE
                    WHEN delivered_ts IS NULL
                     AND processing_started_at IS NULL
                     AND next_retry_at > ?1
                     AND attempts < ?2 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE
                    WHEN delivered_ts IS NULL
                     AND processing_started_at IS NOT NULL THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE
                    WHEN delivered_ts IS NULL
                     AND attempts >= ?2 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE
                    WHEN delivered_ts IS NULL
                     AND last_sync_error IS NOT NULL
                     AND last_sync_error != '' THEN 1 ELSE 0 END), 0)
            FROM metrics
            "#,
            params![now as i64, MAX_METRIC_UPLOAD_ATTEMPTS as i64],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )?;

        let latest_error: Option<String> = self
            .conn
            .query_row(
                "SELECT last_sync_error FROM metrics \
                 WHERE delivered_ts IS NULL \
                   AND last_sync_error IS NOT NULL \
                   AND last_sync_error != '' \
                 ORDER BY COALESCE(last_sync_at, 0) DESC, id DESC \
                 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        Ok(MetricsStatus {
            total: total as usize,
            delivered: delivered as usize,
            not_delivered: not_delivered as usize,
            pending_retryable: pending_retryable as usize,
            waiting_retry: waiting_retry as usize,
            processing: processing as usize,
            stopped_after_errors: stopped_after_errors as usize,
            rows_with_errors: rows_with_errors as usize,
            latest_error,
        })
    }

    /// Get count of pending metrics.
    pub fn count(&self) -> Result<usize, GitAiError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM metrics WHERE delivered_ts IS NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Query persisted metric rows since `since_ts` (Unix seconds).
    ///
    /// When `repo_filter` is `Some(url)`, only events matching that repo_url are returned.
    /// An empty string `""` is a sentinel meaning "events with no repo_url (NULL)".
    /// When `None`, all events are returned regardless of repo.
    pub fn get_metric_history(
        &self,
        since_ts: u32,
        repo_filter: Option<&str>,
        event_ids: &[u16],
    ) -> Result<Vec<MetricHistoryRecord>, GitAiError> {
        use crate::metrics::types::MetricEvent;

        let mut stmt = self
            .conn
            .prepare("SELECT event_json, event_ts, event_kind FROM metrics WHERE event_ts IS NULL OR event_ts >= ?1 ORDER BY id ASC")?;
        let rows = stmt.query_map(params![since_ts as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?;

        let mut records = Vec::new();
        for row in rows {
            let (event_json, _cached_ts, cached_kind) = row?;
            if let Some(kind) = cached_kind
                && (0..=u16::MAX as i64).contains(&kind)
                && !event_ids.contains(&(kind as u16))
            {
                continue;
            }

            let Ok(event) = serde_json::from_str::<MetricEvent>(&event_json) else {
                continue;
            };

            if event.timestamp < since_ts || !event_ids.contains(&event.event_id) {
                continue;
            }

            let repo_url = sparse_get_string(&event.attrs, attr_pos::REPO_URL).flatten();
            let repo_matches = match repo_filter {
                None => true,
                Some("") => repo_url.is_none(),
                Some(filter) => repo_url.as_deref().is_some_and(|url| url.contains(filter)),
            };
            if !repo_matches {
                continue;
            }

            records.push(MetricHistoryRecord {
                event_id: event.event_id,
                ts: event.timestamp,
                repo_url,
                event,
            });
        }

        Ok(records)
    }
}
