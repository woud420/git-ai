//! Daemon communication for bash-tool: watermark queries, snapshot queries,
//! and hook-attempt / session-end signals.

use crate::model::working_log::AgentId;
use crate::operations::daemon::control_api::{BashSnapshotQueryResponse, ControlRequest};
use crate::operations::daemon::{DaemonConfig, send_control_request_with_timeout};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use super::types::StatSnapshot;

// ---------------------------------------------------------------------------
// Daemon socket resolution
// ---------------------------------------------------------------------------

/// Resolve the daemon control socket path, preferring the thread-local test
/// override over the env-based `DaemonConfig`.
pub fn effective_daemon_socket() -> Option<std::path::PathBuf> {
    #[cfg(any(test, feature = "test-support"))]
    {
        use super::TEST_DAEMON_SOCKET;
        let tl = TEST_DAEMON_SOCKET.with(|c| c.borrow().clone());
        if tl.is_some() {
            return tl;
        }
    }
    DaemonConfig::from_env_or_default_paths()
        .ok()
        .map(|c| c.control_socket_path)
}

// ---------------------------------------------------------------------------
// Watermarks
// ---------------------------------------------------------------------------

/// Watermarks returned by the daemon for a single worktree.
pub struct DaemonWatermarks {
    /// Per-file mtime watermarks from scoped checkpoints.
    pub(crate) per_file: HashMap<String, u128>,
    /// Timestamp of the last full (non-scoped) Human checkpoint, if any.
    /// `None` on cold start (daemon has never processed a full checkpoint).
    pub(crate) worktree: Option<u128>,
}

/// Query the daemon for per-file mtime watermarks for a given repository.
///
/// Returns `None` on any failure (daemon not running, socket error, parse
/// error, etc.) for graceful degradation — the caller simply skips the
/// captured-checkpoint path when watermarks are unavailable.
pub fn query_daemon_watermarks(repo_working_dir: &str) -> Option<DaemonWatermarks> {
    let socket = effective_daemon_socket()?;
    if !socket.exists() {
        return None;
    }
    let request = ControlRequest::SnapshotWatermarks {
        repo_working_dir: repo_working_dir.to_string(),
    };
    let response =
        send_control_request_with_timeout(&socket, &request, Duration::from_millis(500)).ok()?;

    if !response.ok {
        tracing::debug!(
            "Daemon watermark query returned error: {}",
            response.error.as_deref().unwrap_or("unknown")
        );
        return None;
    }

    let data = response.data?;
    let per_file: HashMap<String, u128> = data
        .get("watermarks")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let worktree: Option<u128> = data
        .get("worktree_watermark")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    if per_file.is_empty() && worktree.is_none() {
        return None;
    }
    Some(DaemonWatermarks { per_file, worktree })
}

// ---------------------------------------------------------------------------
// Bash snapshot query
// ---------------------------------------------------------------------------

/// Query the daemon for the pre-snapshot stored during `BashSessionStart`.
///
/// Returns `None` if the daemon is not running, the session is not found,
/// or any communication error occurs.
pub fn query_daemon_bash_snapshot(session_id: &str, tool_use_id: &str) -> Option<StatSnapshot> {
    let socket = effective_daemon_socket()?;
    if !socket.exists() {
        return None;
    }
    let request = ControlRequest::BashSnapshotQuery {
        session_id: session_id.to_string(),
        tool_use_id: tool_use_id.to_string(),
    };
    let response =
        send_control_request_with_timeout(&socket, &request, Duration::from_millis(500)).ok()?;

    if !response.ok {
        tracing::debug!(
            "Daemon bash snapshot query returned error: {}",
            response.error.as_deref().unwrap_or("unknown")
        );
        return None;
    }

    let data = response.data?;
    let snapshot_response: BashSnapshotQueryResponse = serde_json::from_value(data).ok()?;
    snapshot_response.stat_snapshot
}

// ---------------------------------------------------------------------------
// Hook attempt signals
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum BashHookAttemptPhase {
    Start,
    End,
}

pub struct BashHookAttemptSignal<'a> {
    pub original_cwd: &'a Path,
    pub discovered_repo_work_dir: Option<&'a Path>,
    pub repo_discovery_error: Option<&'a str>,
    pub session_id: &'a str,
    pub tool_use_id: &'a str,
    pub agent_id: &'a AgentId,
    pub metadata: &'a HashMap<String, String>,
    pub trace_id: &'a str,
    pub timestamp_ns: u128,
    pub command: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct BashToolHookContext<'a> {
    pub session_id: &'a str,
    pub tool_use_id: &'a str,
    pub agent_id: &'a AgentId,
    pub agent_metadata: Option<&'a HashMap<String, String>>,
    pub trace_id: &'a str,
    pub command: Option<&'a str>,
}

pub fn signal_daemon_bash_hook_attempt(
    phase: BashHookAttemptPhase,
    signal: BashHookAttemptSignal<'_>,
) {
    let Some(socket) = effective_daemon_socket() else {
        return;
    };
    if !socket.exists() {
        return;
    }
    let original_cwd = signal.original_cwd.to_string_lossy().to_string();
    let discovered_repo_work_dir = signal
        .discovered_repo_work_dir
        .map(|path| path.to_string_lossy().to_string());
    let repo_discovery_error = signal.repo_discovery_error.map(ToString::to_string);
    let session_id = signal.session_id.to_string();
    let tool_use_id = signal.tool_use_id.to_string();
    let agent_id = signal.agent_id.clone();
    let metadata = signal.metadata.clone();
    let trace_id = signal.trace_id.to_string();
    let command = signal.command.map(ToString::to_string);

    let request = match phase {
        BashHookAttemptPhase::Start => ControlRequest::BashHookAttemptStart {
            original_cwd,
            discovered_repo_work_dir,
            repo_discovery_error,
            session_id,
            tool_use_id,
            agent_id,
            metadata,
            trace_id,
            started_at_ns: signal.timestamp_ns,
            command,
        },
        BashHookAttemptPhase::End => ControlRequest::BashHookAttemptEnd {
            original_cwd,
            discovered_repo_work_dir,
            repo_discovery_error,
            session_id,
            tool_use_id,
            agent_id,
            metadata,
            trace_id,
            ended_at_ns: signal.timestamp_ns,
            command,
        },
    };
    if let Err(e) = send_control_request_with_timeout(&socket, &request, Duration::from_millis(500))
    {
        tracing::debug!("Failed to signal bash hook attempt {:?}: {}", phase, e);
    }
}

// ---------------------------------------------------------------------------
// Session end signal
// ---------------------------------------------------------------------------

pub(super) struct BashSessionEndSignal<'a> {
    pub(super) repo_work_dir: &'a str,
    pub(super) original_cwd: &'a Path,
    pub(super) session_id: &'a str,
    pub(super) tool_use_id: &'a str,
    pub(super) agent_id: &'a AgentId,
    pub(super) metadata: &'a HashMap<String, String>,
    pub(super) trace_id: &'a str,
    pub(super) ended_at_ns: u128,
    pub(super) command: Option<&'a str>,
}

/// Signal the daemon that a bash session has ended.
pub(super) fn signal_daemon_bash_session_end(signal: BashSessionEndSignal<'_>) {
    let Some(socket) = effective_daemon_socket() else {
        return;
    };
    if !socket.exists() {
        return;
    }
    let request = ControlRequest::BashSessionEnd {
        repo_work_dir: signal.repo_work_dir.to_string(),
        original_cwd: Some(signal.original_cwd.to_string_lossy().to_string()),
        session_id: signal.session_id.to_string(),
        tool_use_id: signal.tool_use_id.to_string(),
        agent_id: signal.agent_id.clone(),
        metadata: signal.metadata.clone(),
        trace_id: signal.trace_id.to_string(),
        ended_at_ns: signal.ended_at_ns,
        command: signal.command.map(ToString::to_string),
    };
    if let Err(e) = send_control_request_with_timeout(&socket, &request, Duration::from_millis(500))
    {
        tracing::debug!("Failed to signal bash session end: {}", e);
    }
}
