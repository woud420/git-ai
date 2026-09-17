/// A candidate session event row used during bash-mtime attribution recovery.
///
/// Produced by `MetricsDatabase` queries and consumed by
/// `operations::authorship::attribution_recovery`. Defined here (outside
/// `metrics_db`) so that neither the persistence nor the recovery layer owns
/// the other's vocabulary.
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

pub(crate) const NS_PER_SECOND: u128 = 1_000_000_000;

pub(crate) fn distance_to_event_second(timestamp_ns: u128, event_ts: u32) -> u128 {
    let start_ns = event_ts as u128 * NS_PER_SECOND;
    let end_ns = start_ns.saturating_add(NS_PER_SECOND - 1);
    if timestamp_ns < start_ns {
        start_ns - timestamp_ns
    } else {
        timestamp_ns.saturating_sub(end_ns)
    }
}
