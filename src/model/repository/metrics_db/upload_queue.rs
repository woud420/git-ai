use crate::error::GitAiError;
use rusqlite::params;

use super::MetricsDatabase;
use super::schema::MAX_METRIC_UPLOAD_ATTEMPTS;

pub(crate) const METRIC_PROCESSING_LOCK_TIMEOUT_SECS: u64 = 10 * 60;

impl MetricsDatabase {
    /// Mark records as delivered after a successful upload.
    pub fn mark_records_delivered(
        &mut self,
        ids: &[i64],
        delivered_ts: u64,
    ) -> Result<(), GitAiError> {
        if ids.is_empty() {
            return Ok(());
        }

        let tx = self.conn.transaction()?;

        {
            let mut stmt = tx.prepare_cached(
                "UPDATE metrics \
                 SET delivered_ts = ?1, processing_started_at = NULL \
                 WHERE id = ?2 AND delivered_ts IS NULL",
            )?;

            for id in ids {
                stmt.execute(params![delivered_ts as i64, id])?;
            }
        }

        tx.commit()?;
        self.prune_old_metrics_if_due()?;
        Ok(())
    }

    /// Mark records as failed and schedule their next row-level retry.
    pub fn mark_records_failed(
        &mut self,
        ids: &[i64],
        error: &str,
        failed_at: u64,
    ) -> Result<(), GitAiError> {
        if ids.is_empty() {
            return Ok(());
        }

        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                r#"
                UPDATE metrics
                SET processing_started_at = NULL,
                    attempts = attempts + 1,
                    last_sync_error = ?1,
                    last_sync_at = ?2,
                    next_retry_at = ?2 + CASE
                        WHEN attempts + 1 <= 1 THEN 300
                        WHEN attempts + 1 = 2 THEN 1800
                        WHEN attempts + 1 = 3 THEN 7200
                        WHEN attempts + 1 = 4 THEN 21600
                        WHEN attempts + 1 = 5 THEN 43200
                        ELSE 86400
                    END
                WHERE id = ?3 AND delivered_ts IS NULL
                "#,
            )?;

            for id in ids {
                stmt.execute(params![error, failed_at as i64, id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Mark records as permanently undeliverable while retaining them in history.
    pub fn mark_records_undeliverable(
        &mut self,
        records: &[(i64, String)],
        failed_at: u64,
    ) -> Result<(), GitAiError> {
        if records.is_empty() {
            return Ok(());
        }

        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE metrics \
                 SET processing_started_at = NULL, \
                     attempts = ?1, \
                     last_sync_error = ?2, \
                     last_sync_at = ?3, \
                     next_retry_at = ?3 \
                 WHERE id = ?4 AND delivered_ts IS NULL",
            )?;

            for (id, error) in records {
                stmt.execute(params![
                    MAX_METRIC_UPLOAD_ATTEMPTS as i64,
                    error,
                    failed_at as i64,
                    id
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(super) fn release_stale_processing_locks(&mut self, now: u64) -> Result<(), GitAiError> {
        let stale_before = now.saturating_sub(METRIC_PROCESSING_LOCK_TIMEOUT_SECS);
        self.conn.execute(
            "UPDATE metrics \
             SET processing_started_at = NULL \
             WHERE delivered_ts IS NULL \
               AND processing_started_at IS NOT NULL \
               AND processing_started_at < ?1",
            params![stale_before as i64],
        )?;
        Ok(())
    }
}

pub(super) fn current_unix_ts() -> u64 {
    crate::model::clock::now_secs()
}
