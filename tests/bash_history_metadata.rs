#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use git_ai::model::daemon_control::ControlRequest;
use git_ai::model::repository::bash_history_db::BashHistoryDatabase;
use git_ai::model::working_log::AgentId;
use git_ai::operations::daemon::send_control_request;
use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::collections::HashMap;
use std::fs;

#[test]
fn bash_session_completion_preserves_start_only_metadata() {
    assert_completion_metadata(true);
}

#[test]
fn bash_hook_completion_preserves_start_only_metadata() {
    assert_completion_metadata(false);
}

fn assert_completion_metadata(session: bool) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bash.sqlite");
    let repo = TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH",
        path.to_str().unwrap(),
    )]);
    fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();
    repo.filename("base.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    let cwd = repo.canonical_path().to_string_lossy().into_owned();
    let agent = AgentId {
        tool: "codex".into(),
        id: "metadata-session".into(),
        model: "test-model".into(),
    };
    let metadata = HashMap::from([
        ("start_only".to_owned(), "retain me".to_owned()),
        ("shared".to_owned(), "old".to_owned()),
    ]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let start = if session {
        ControlRequest::BashSessionStart {
            repo_work_dir: cwd.clone(),
            original_cwd: Some(cwd.clone()),
            session_id: "metadata-session".into(),
            tool_use_id: "tool".into(),
            agent_id: agent.clone(),
            metadata,
            stat_snapshot: Box::new(git_ai::model::stat_snapshot::StatSnapshot {
                entries: HashMap::new(),
                taken_at: None,
                invocation_key: "metadata-session:tool".into(),
                repo_root: repo.path().to_path_buf(),
                effective_worktree_wm: None,
                per_file_wm: HashMap::new(),
            }),
            trace_id: "start".into(),
            started_at_ns: now,
            command: Some("true".into()),
        }
    } else {
        ControlRequest::BashHookAttemptStart {
            original_cwd: cwd.clone(),
            discovered_repo_work_dir: Some(cwd.clone()),
            repo_discovery_error: None,
            session_id: "metadata-session".into(),
            tool_use_id: "tool".into(),
            agent_id: agent.clone(),
            metadata,
            trace_id: "start".into(),
            started_at_ns: now,
            command: Some("true".into()),
        }
    };
    let response = send_control_request(&repo.daemon_control_socket_path(), &start).unwrap();
    assert!(response.ok, "{response:?}");
    let metadata = HashMap::from([
        ("end_only".to_owned(), "new field".to_owned()),
        ("shared".to_owned(), "updated".to_owned()),
    ]);
    let end = if session {
        ControlRequest::BashSessionEnd {
            repo_work_dir: cwd.clone(),
            original_cwd: Some(cwd),
            session_id: "metadata-session".into(),
            tool_use_id: "tool".into(),
            agent_id: agent,
            metadata,
            trace_id: "end".into(),
            ended_at_ns: now + 1_000_000,
            command: None,
        }
    } else {
        ControlRequest::BashHookAttemptEnd {
            original_cwd: cwd.clone(),
            discovered_repo_work_dir: Some(cwd),
            repo_discovery_error: None,
            session_id: "metadata-session".into(),
            tool_use_id: "tool".into(),
            agent_id: agent,
            metadata,
            trace_id: "end".into(),
            ended_at_ns: now + 1_000_000,
            command: None,
        }
    };
    let response = send_control_request(&repo.daemon_control_socket_path(), &end).unwrap();
    assert!(response.ok, "{response:?}");
    let db = BashHistoryDatabase::open_at_path(&path).unwrap();
    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].start_trace_id.as_deref(), Some("start"));
    assert_eq!(calls[0].end_trace_id.as_deref(), Some("end"));
    assert_eq!(
        calls[0].metadata,
        HashMap::from([
            ("start_only".to_owned(), "retain me".to_owned()),
            ("end_only".to_owned(), "new field".to_owned()),
            ("shared".to_owned(), "updated".to_owned()),
        ])
    );
}
