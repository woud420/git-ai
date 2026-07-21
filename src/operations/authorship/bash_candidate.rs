//! Domain candidate type for bash-mtime attribution recovery.
//!
//! `BashCandidate` is the domain representation of a bash checkpoint call
//! used during recovery selection. It deliberately excludes DB-only fields
//! (`invocation_key`, trace IDs) and replaces the raw autoincrement `id`
//! with an explicit `recency_ordinal` so call sites are not coupled to the
//! storage row ordering.

use crate::model::repository::bash_history_db::BashCheckpointCall;
use crate::model::working_log::AgentId;

/// Domain-level representation of a bash checkpoint call used for recovery candidate
/// selection. Fields are sufficient to select a winner, attribute lines, and emit
/// a recovery metric — no storage-layer details (row id, trace ids, invocation key)
/// are exposed.
pub(crate) struct BashCandidate {
    /// Ordinal derived from the SQLite autoincrement rowid: higher = more recent.
    /// Used as a stable tiebreaker in `select_best_bash_candidate` so the same
    /// winner is chosen regardless of field ordering.
    pub recency_ordinal: i64,
    /// Identity of the AI agent that ran the bash command.
    pub agent_id: AgentId,
    /// Working directory of the process that ran the bash command.
    pub original_cwd: String,
    /// Resolved git worktree root at checkpoint time, if available.
    pub repo_work_dir: Option<String>,
    /// Error from repo discovery, if the workdir could not be resolved.
    pub repo_discovery_error: Option<String>,
    /// The tool-use ID of the bash call within its session.
    pub tool_use_id: String,
    /// Nanosecond timestamp when the bash command started.
    pub start_time_ns: u128,
    /// Nanosecond timestamp when the bash command ended, or `None` if still running.
    pub end_time_ns: Option<u128>,
    /// The command that was executed, if captured.
    pub command: Option<String>,
}

impl From<BashCheckpointCall> for BashCandidate {
    fn from(call: BashCheckpointCall) -> Self {
        BashCandidate {
            recency_ordinal: call.id,
            agent_id: call.agent_id,
            original_cwd: call.original_cwd,
            repo_work_dir: call.repo_work_dir,
            repo_discovery_error: call.repo_discovery_error,
            tool_use_id: call.tool_use_id,
            start_time_ns: call.start_time_ns,
            end_time_ns: call.end_time_ns,
            command: call.command,
        }
    }
}

/// Distance (in nanoseconds) between `timestamp_ns` and the call's execution window
/// `[start_time_ns, end_time_ns]`. Returns `0` when the timestamp falls inside the
/// window.
///
/// This function lives here rather than in `bash_history_db` because it expresses
/// domain matching logic, not a persistence concern. `bash_history_db` retains a
/// thin wrapper for its internal candidate pre-filter query.
pub(crate) fn distance_to_call_window(timestamp_ns: u128, call: &BashCandidate) -> u128 {
    let start = call.start_time_ns;
    let end = call.end_time_ns.unwrap_or(start);
    if timestamp_ns < start {
        start.saturating_sub(timestamp_ns)
    } else if timestamp_ns > end {
        timestamp_ns.saturating_sub(end)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(start_ns: u128, end_ns: Option<u128>) -> BashCandidate {
        BashCandidate {
            recency_ordinal: 1,
            agent_id: AgentId {
                tool: "codex".into(),
                id: "s1".into(),
                model: "gpt-5".into(),
            },
            original_cwd: "/repo".into(),
            repo_work_dir: Some("/repo".into()),
            repo_discovery_error: None,
            tool_use_id: "t1".into(),
            start_time_ns: start_ns,
            end_time_ns: end_ns,
            command: None,
        }
    }

    #[test]
    fn distance_inside_window_is_zero() {
        let c = candidate(100, Some(200));
        assert_eq!(distance_to_call_window(150, &c), 0);
    }

    #[test]
    fn distance_before_window() {
        let c = candidate(100, Some(200));
        assert_eq!(distance_to_call_window(80, &c), 20);
    }

    #[test]
    fn distance_after_window() {
        let c = candidate(100, Some(200));
        assert_eq!(distance_to_call_window(250, &c), 50);
    }

    #[test]
    fn distance_at_window_boundary_is_zero() {
        let c = candidate(100, Some(200));
        assert_eq!(distance_to_call_window(100, &c), 0);
        assert_eq!(distance_to_call_window(200, &c), 0);
    }

    #[test]
    fn distance_no_end_uses_start_as_end() {
        // When end_time_ns is None the window collapses to a single point.
        let c = candidate(100, None);
        assert_eq!(distance_to_call_window(100, &c), 0);
        assert_eq!(distance_to_call_window(50, &c), 50);
        assert_eq!(distance_to_call_window(120, &c), 20);
    }

    #[test]
    fn from_bash_checkpoint_call_maps_id_to_recency_ordinal() {
        use crate::model::repository::bash_history_db::BashCheckpointCall;
        use std::collections::HashMap;

        let call = BashCheckpointCall {
            id: 42,
            invocation_key: "s:t".into(),
            original_cwd: "/home/user".into(),
            repo_work_dir: Some("/repo".into()),
            repo_discovery_error: None,
            session_id: "ext-session".into(),
            tool_use_id: "tool-1".into(),
            agent_id: AgentId {
                tool: "codex".into(),
                id: "ext-session".into(),
                model: "gpt-4".into(),
            },
            start_trace_id: Some("t_start".into()),
            end_trace_id: Some("t_end".into()),
            start_time_ns: 1_000,
            end_time_ns: Some(2_000),
            command: Some("echo hi".into()),
            metadata: HashMap::new(),
        };

        let candidate = BashCandidate::from(call);
        assert_eq!(candidate.recency_ordinal, 42);
        assert_eq!(candidate.original_cwd, "/home/user");
        assert_eq!(candidate.repo_work_dir, Some("/repo".into()));
        assert_eq!(candidate.tool_use_id, "tool-1");
        assert_eq!(candidate.start_time_ns, 1_000);
        assert_eq!(candidate.end_time_ns, Some(2_000));
        assert_eq!(candidate.command, Some("echo hi".into()));
    }
}
