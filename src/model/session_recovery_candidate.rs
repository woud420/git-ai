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
