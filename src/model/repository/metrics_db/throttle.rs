use crate::error::GitAiError;
use rusqlite::{OptionalExtension, params};

use super::MetricsDatabase;

impl MetricsDatabase {
    /// Returns whether an `agent_usage` event should be emitted for this prompt_id.
    ///
    /// If emitted, this method also updates the prompt's last-sent timestamp.
    pub fn should_emit_agent_usage(
        &mut self,
        prompt_id: &str,
        now_ts: u64,
        min_interval_secs: u64,
    ) -> Result<bool, GitAiError> {
        if prompt_id.is_empty() {
            return Ok(true);
        }

        let tx = self.conn.transaction()?;
        let existing_ts: Option<i64> = tx
            .query_row(
                "SELECT last_sent_ts FROM agent_usage_throttle WHERE prompt_id = ?1",
                params![prompt_id],
                |row| row.get(0),
            )
            .optional()?;

        let should_emit = existing_ts
            .map(|prev_ts| now_ts.saturating_sub(prev_ts as u64) >= min_interval_secs)
            .unwrap_or(true);

        if should_emit {
            tx.execute(
                r#"
                INSERT INTO agent_usage_throttle (prompt_id, last_sent_ts)
                VALUES (?1, ?2)
                ON CONFLICT(prompt_id) DO UPDATE SET last_sent_ts = excluded.last_sent_ts
                "#,
                params![prompt_id, now_ts as i64],
            )?;
        }

        tx.commit()?;
        Ok(should_emit)
    }
}
