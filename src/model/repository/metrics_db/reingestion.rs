use crate::error::GitAiError;
use rusqlite::params;

use super::MetricsDatabase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetricsReingestScope {
    All,
    Range { from_ts: u32, to_ts: u32 },
}

impl MetricsReingestScope {
    pub(crate) fn from_bounds(
        from_ts: Option<u32>,
        to_ts: Option<u32>,
    ) -> Result<Self, &'static str> {
        match (from_ts, to_ts) {
            (None, None) => Ok(Self::All),
            (Some(from_ts), Some(to_ts)) if from_ts < to_ts => Ok(Self::Range { from_ts, to_ts }),
            _ => Err("metrics reingestion requires no bounds or an ordered from/to pair"),
        }
    }
}

impl MetricsDatabase {
    /// Reset matching rows so the telemetry worker can deliver them again.
    ///
    /// Ranges are half-open event-time intervals. Reusing retained rows keeps
    /// local history deduplicated and makes a repeated request safe.
    pub(crate) fn reingest_metrics(
        &mut self,
        scope: MetricsReingestScope,
    ) -> Result<usize, GitAiError> {
        let (from_ts, to_ts) = match scope {
            MetricsReingestScope::All => (None, None),
            MetricsReingestScope::Range { from_ts, to_ts } => {
                if !self.event_metadata_backfill_completed()? {
                    self.backfill_event_metadata()?;
                }
                (Some(i64::from(from_ts)), Some(i64::from(to_ts)))
            }
        };

        self.conn
            .execute(
                r#"
                UPDATE metrics
                SET delivered_ts = NULL,
                    attempts = 0,
                    last_sync_error = NULL,
                    last_sync_at = NULL,
                    next_retry_at = 0,
                    processing_started_at = NULL
                WHERE processing_started_at IS NULL
                  AND (
                    (?1 IS NULL AND ?2 IS NULL)
                    OR (event_ts >= ?1 AND event_ts < ?2 AND event_kind IS NOT NULL)
                  )
                "#,
                params![from_ts, to_ts],
            )
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::repository::metrics_db::test_support::{
        create_test_db, event_json, seconds_ago, unix_now,
    };

    #[test]
    fn range_resets_delivery_state_in_a_half_open_interval() {
        let (mut db, _temp_dir) = create_test_db();
        let now = unix_now();
        let from_ts = seconds_ago(200);
        let to_ts = seconds_ago(100);
        let ids = db
            .insert_events_with_delivered_ts(
                &[
                    event_json(from_ts - 1),
                    event_json(from_ts),
                    event_json(from_ts + 50),
                    event_json(to_ts - 1),
                    event_json(to_ts),
                ],
                Some(now),
            )
            .unwrap();
        db.conn
            .execute(
                "UPDATE metrics SET attempts = 6, last_sync_error = 'stopped', \
                 last_sync_at = ?1, next_retry_at = ?1",
                params![now as i64],
            )
            .unwrap();
        db.conn
            .execute(
                "UPDATE metrics SET processing_started_at = ?1 WHERE id = ?2",
                params![now as i64, ids[1]],
            )
            .unwrap();
        db.conn
            .execute(
                "UPDATE metrics SET event_ts = NULL, event_kind = NULL WHERE id = ?1",
                params![ids[2]],
            )
            .unwrap();

        let reset = db
            .reingest_metrics(MetricsReingestScope::Range { from_ts, to_ts })
            .unwrap();

        assert_eq!(reset, 2);
        assert!(db.event_metadata_backfill_completed().unwrap());
        let states = db
            .conn
            .prepare(
                "SELECT delivered_ts, attempts, last_sync_error, last_sync_at, \
                 next_retry_at, processing_started_at FROM metrics ORDER BY id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        for (index, state) in states.iter().enumerate() {
            if matches!(index, 2..=3) {
                assert_eq!(state, &(None, 0, None, None, 0, None));
            } else {
                assert_eq!(state.0, Some(now as i64));
                assert_eq!(state.1, 6);
                assert_eq!(state.2.as_deref(), Some("stopped"));
                assert_eq!(state.3, Some(now as i64));
                assert_eq!(state.4, now as i64);
                assert_eq!(state.5, (index == 1).then_some(now as i64));
            }
        }
    }

    #[test]
    fn all_resets_malformed_rows_without_creating_duplicates() {
        let (mut db, _temp_dir) = create_test_db();
        let now = unix_now();
        db.insert_events_with_delivered_ts(
            &[event_json(seconds_ago(100)), "not-json".to_string()],
            Some(now),
        )
        .unwrap();

        assert_eq!(db.reingest_metrics(MetricsReingestScope::All).unwrap(), 2);
        assert_eq!(db.status().unwrap().total, 2);
        assert_eq!(db.status().unwrap().pending_retryable, 2);
    }

    #[test]
    fn scope_requires_both_ordered_bounds_or_neither() {
        assert_eq!(
            MetricsReingestScope::from_bounds(None, None),
            Ok(MetricsReingestScope::All)
        );
        assert!(MetricsReingestScope::from_bounds(Some(1), None).is_err());
        assert!(MetricsReingestScope::from_bounds(None, Some(2)).is_err());
        assert!(MetricsReingestScope::from_bounds(Some(2), Some(2)).is_err());
        assert!(MetricsReingestScope::from_bounds(Some(3), Some(2)).is_err());
    }
}
