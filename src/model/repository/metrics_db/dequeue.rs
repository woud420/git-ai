use super::{MetricsDatabase, types::MetricRecord, upload_queue::current_unix_ts};
use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use rusqlite::params;

pub(crate) const RETRYABLE_METRIC_IDS_SQL: &str = "SELECT id, LENGTH(CAST(event_json AS BLOB)) FROM metrics \
     WHERE delivered_ts IS NULL \
       AND processing_started_at IS NULL \
       AND next_retry_at <= ?1 \
       AND attempts < 6 \
     ORDER BY next_retry_at ASC, id DESC \
     LIMIT ?2";

impl MetricsDatabase {
    /// Atomically claim a due batch of pending metrics for upload.
    pub fn dequeue_pending_batch(&mut self, limit: usize) -> Result<Vec<MetricRecord>, GitAiError> {
        self.dequeue_pending_batch_with_byte_limit(limit, usize::MAX)
    }

    /// Measure payload bytes before claiming or materializing any JSON records.
    pub fn dequeue_pending_batch_with_byte_limit(
        &mut self,
        limit: usize,
        max_bytes: usize,
    ) -> Result<Vec<MetricRecord>, GitAiError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let now = current_unix_ts();
        self.release_stale_processing_locks(now)?;

        let tx = self.conn.transaction()?;
        let ids = {
            let mut stmt = tx.prepare(RETRYABLE_METRIC_IDS_SQL)?;
            let mut rows = stmt.query(params![now as i64, limit.min(i64::MAX as usize) as i64])?;
            let mut ids = Vec::new();
            let mut remaining = max_bytes as u64;
            while let Some(row) = rows.next()? {
                let id: i64 = row.get(0)?;
                let bytes: u64 = row.get(1)?;
                if bytes > remaining {
                    if ids.is_empty() {
                        return Err(PersistenceError::ReadBudgetExceeded {
                            resource: format!(
                                "metrics record {id} (max_metrics_flush_chunk_bytes)"
                            ),
                            actual_bytes: bytes,
                            limit_bytes: max_bytes as u64,
                        }
                        .into());
                    }
                    break;
                }
                remaining -= bytes;
                ids.push(id);
            }
            ids
        };

        if ids.is_empty() {
            tx.commit()?;
            return Ok(Vec::new());
        }

        let mut locked_ids = Vec::with_capacity(ids.len());
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE metrics \
                 SET processing_started_at = ?1 \
                 WHERE id = ?2 \
                   AND delivered_ts IS NULL \
                   AND processing_started_at IS NULL",
            )?;
            for id in ids {
                if stmt.execute(params![now as i64, id])? > 0 {
                    locked_ids.push(id);
                }
            }
        }

        let mut records = Vec::with_capacity(locked_ids.len());
        {
            let mut stmt = tx.prepare_cached(
                "SELECT id, event_json, attempts, next_retry_at FROM metrics WHERE id = ?1",
            )?;
            for id in locked_ids {
                records.push(stmt.query_row(params![id], |row| {
                    Ok(MetricRecord {
                        id: row.get(0)?,
                        event_json: row.get(1)?,
                        attempts: row.get::<_, i64>(2)?.max(0) as u32,
                        next_retry_at: row.get::<_, i64>(3)?.max(0) as u64,
                    })
                })?);
            }
        }

        tx.commit()?;
        Ok(records)
    }
}
