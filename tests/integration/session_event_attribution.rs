use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::{CodexHookInput, checkpoint_codex, isolated_bash_history_db_path};
use git_ai::metrics::{EventAttributes, MetricEvent, PosEncoded, SessionEventValues};
use git_ai::model::authorship_log::LineRange;
use git_ai::model::authorship_log_serialization::{AuthorshipLog, generate_session_id};
use git_ai::model::repository::bash_history_db::{BashCallEnd, BashCallStart, BashHistoryDatabase};
use git_ai::model::repository::metrics_db::MetricsDatabase;
use git_ai::model::working_log::AgentId;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

fn isolated_metrics_db_path() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("failed to create isolated metrics db dir");
    let path = dir.path().join("metrics.db");
    (dir, path.to_string_lossy().to_string())
}

fn file_mtime_secs(path: &Path) -> u32 {
    fs::metadata(path)
        .expect("file metadata should be readable")
        .modified()
        .expect("file mtime should be readable")
        .duration_since(UNIX_EPOCH)
        .expect("file mtime should be after epoch")
        .as_secs()
        .min(u32::MAX as u64) as u32
}

fn insert_session_event(
    db_path: &str,
    event_ts: u32,
    external_session_id: &str,
    external_tool_use_id: &str,
    repo_url: Option<&str>,
) -> String {
    insert_session_event_for_tool(
        db_path,
        event_ts,
        "codex",
        external_session_id,
        external_tool_use_id,
        repo_url,
    )
}

fn insert_session_event_for_tool(
    db_path: &str,
    event_ts: u32,
    tool: &str,
    external_session_id: &str,
    external_tool_use_id: &str,
    repo_url: Option<&str>,
) -> String {
    let session_id = generate_session_id(external_session_id, tool);
    let values = SessionEventValues::with_ids(
        json!({
            "type": "assistant",
            "session_id": external_session_id,
        }),
        Some(format!("event-{external_tool_use_id}")),
        None,
        Some(external_tool_use_id.to_string()),
    );
    let mut attrs = EventAttributes::with_version("test")
        .tool(tool)
        .model("gpt-5")
        .external_session_id(external_session_id)
        .session_id(&session_id)
        .trace_id(format!("trace-{external_tool_use_id}"));
    if let Some(repo_url) = repo_url {
        attrs = attrs.repo_url(repo_url);
    }
    let event = MetricEvent::from_values_with_timestamp(values, attrs.to_sparse(), Some(event_ts));
    let event_json = serde_json::to_string(&event).expect("metric event should serialize");

    let mut db = MetricsDatabase::open_at_path(Path::new(db_path))
        .expect("metrics db should open at isolated path");
    db.insert_events(&[event_json])
        .expect("session event should insert");

    session_id
}

fn attested_lines_for_session(
    authorship_log: &AuthorshipLog,
    file_path: &str,
    session_id: &str,
) -> Vec<u32> {
    let mut lines = authorship_log
        .attestations
        .iter()
        .filter(|attestation| attestation.file_path == file_path)
        .flat_map(|attestation| &attestation.entries)
        .filter(|entry| entry.hash.split("::").next() == Some(session_id))
        .flat_map(|entry| entry.line_ranges.iter().flat_map(LineRange::expand))
        .collect::<Vec<_>>();
    lines.sort_unstable();
    lines.dedup();
    lines
}

fn assert_session_attests_lines(
    authorship_log: &AuthorshipLog,
    file_path: &str,
    session_id: &str,
    expected_lines: &[u32],
) {
    assert_eq!(
        attested_lines_for_session(authorship_log, file_path, session_id),
        expected_lines,
        "expected {session_id} to attest lines {expected_lines:?}"
    );
}

fn session_ids_for_tool(authorship_log: &AuthorshipLog, tool: &str) -> Vec<String> {
    let mut session_ids = authorship_log
        .metadata
        .sessions
        .iter()
        .filter(|(_, session)| session.agent_id.tool == tool)
        .map(|(session_id, _)| session_id.clone())
        .collect::<Vec<_>>();
    session_ids.sort();
    session_ids
}

fn insert_bash_call(
    db_path: &str,
    repo_work_dir: &str,
    timestamp_secs: u32,
    external_session_id: &str,
    tool_use_id: &str,
) -> String {
    let tool = "codex";
    let session_id = generate_session_id(external_session_id, tool);
    let mut db = BashHistoryDatabase::open_at_path(Path::new(db_path))
        .expect("bash history db should open at isolated path");
    let start_ns = u128::from(timestamp_secs).saturating_mul(1_000_000_000);
    let end_ns = start_ns.saturating_add(1_000_000_000);
    let agent_id = AgentId {
        tool: tool.to_string(),
        id: external_session_id.to_string(),
        model: "gpt-5".to_string(),
    };
    db.record_start(&BashCallStart {
        original_cwd: repo_work_dir.to_string(),
        repo_work_dir: Some(repo_work_dir.to_string()),
        repo_discovery_error: None,
        session_id: external_session_id.to_string(),
        tool_use_id: tool_use_id.to_string(),
        agent_id: agent_id.clone(),
        start_trace_id: format!("trace-start-{tool_use_id}"),
        started_at_ns: start_ns,
        command: Some("codex exec".to_string()),
        metadata: HashMap::new(),
    })
    .expect("bash start should insert");
    db.record_end(&BashCallEnd {
        original_cwd: repo_work_dir.to_string(),
        repo_work_dir: Some(repo_work_dir.to_string()),
        repo_discovery_error: None,
        session_id: external_session_id.to_string(),
        tool_use_id: tool_use_id.to_string(),
        agent_id,
        start_trace_id: Some(format!("trace-start-{tool_use_id}")),
        end_trace_id: format!("trace-end-{tool_use_id}"),
        started_at_ns: Some(start_ns),
        ended_at_ns: end_ns,
        command: Some("codex exec".to_string()),
        metadata: HashMap::new(),
    })
    .expect("bash end should insert");

    session_id
}

mod commit_identity_recovery;
mod time_window_recovery;
