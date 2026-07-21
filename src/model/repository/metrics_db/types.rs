use crate::metrics::types::MetricEvent;

/// Record returned from database queries
#[derive(Debug, Clone)]
pub struct MetricRecord {
    pub id: i64,
    pub event_json: String,
    pub attempts: u32,
    pub next_retry_at: u64,
}

/// Record returned for local usage aggregation from the metrics table.
#[derive(Debug, Clone)]
pub struct MetricHistoryRecord {
    pub event_id: u16,
    pub ts: u32,
    pub repo_url: Option<String>,
    pub event: MetricEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionEventRecoveryCandidate {
    pub row_id: i64,
    pub event_ts: u32,
    pub session_id: String,
    pub trace_id: Option<String>,
    pub tool: String,
    pub model: Option<String>,
    pub external_session_id: String,
    pub external_tool_use_id: Option<String>,
    pub repo_url: Option<String>,
}

/// Point-in-time status summary for local metric delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsStatus {
    pub total: usize,
    pub delivered: usize,
    pub not_delivered: usize,
    pub pending_retryable: usize,
    pub waiting_retry: usize,
    pub processing: usize,
    pub stopped_after_errors: usize,
    pub rows_with_errors: usize,
    pub latest_error: Option<String>,
}

/// Summary returned by event metadata backfill work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetricMetadataBackfillSummary {
    pub scanned: usize,
    pub updated: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MetricEventMetadata {
    pub event_ts: u32,
    pub event_kind: u16,
    pub trace_id: Option<String>,
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub tool: Option<String>,
    pub external_session_id: Option<String>,
    pub external_parent_session_id: Option<String>,
    pub external_event_id: Option<String>,
    pub external_parent_event_id: Option<String>,
    pub external_tool_use_id: Option<String>,
}
