//! Metrics storage for local history and offline buffering.
//!
//! Every metric event is stored here. `delivered_ts IS NULL` means the row is
//! still pending upload; delivered rows are retained as the local history.
//! Server handles idempotency.

use rusqlite::Connection;

mod backfill;
mod event_writes;
mod recovery_queries;
mod schema;
mod status_queries;
mod throttle;
mod types;
mod upload_queue;

pub(crate) use event_writes::METADATA_BACKFILL_BATCH_SIZE;
pub(crate) use types::SessionEventRecoveryCandidate;
pub use types::{MetricHistoryRecord, MetricMetadataBackfillSummary, MetricRecord, MetricsStatus};

// Re-exports used by integration and unit tests within this module tree.
#[cfg(test)]
pub(crate) use recovery_queries::NS_PER_SECOND;
#[cfg(test)]
pub(crate) use schema::MAX_METRIC_UPLOAD_ATTEMPTS;
#[cfg(test)]
pub(crate) use upload_queue::{METRIC_PROCESSING_LOCK_TIMEOUT_SECS, RETRYABLE_METRIC_IDS_SQL};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_backfill;
#[cfg(test)]
mod tests_event_writes;
#[cfg(test)]
mod tests_recovery_queries;
#[cfg(test)]
mod tests_schema;
#[cfg(test)]
mod tests_status_queries;
#[cfg(test)]
mod tests_upload_queue;

/// Database wrapper for metrics storage
pub struct MetricsDatabase {
    conn: Connection,
}
