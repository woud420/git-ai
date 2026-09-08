use crate::commit_metric_metadata::{isolated_metrics_db_path, sparse_str};
use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::{
    CodexHookInput, checkpoint_codex, fixture_path, isolated_bash_history_db_path,
};
use git_ai::metrics::MetricEvent;
use git_ai::metrics::attrs::attr_pos;
use git_ai::metrics::events::checkpoint_pos;
use git_ai::metrics::types::MetricEventId;
use git_ai::model::repository::bash_history_db::BashHistoryDatabase;
use git_ai::model::repository::metrics_db::MetricsDatabase;
use git_ai::model::working_log::AgentId;
use git_ai::operations::commands::checkpoint_agent::bash_tool::{
    BashCheckpointAction, handle_bash_post_tool_use, handle_bash_pre_tool_use_with_context,
    reset_timeout_overrides_for_test, set_daemon_socket_for_test, set_walk_timeout_ms_for_test,
};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

fn claude_edge_file_edit_checkpoint(
    repo: &TestRepo,
    file_path: &Path,
    transcript_path: &Path,
    hook_event_name: &str,
    external_session_id: &str,
) {
    let hook_input = json!({
        "cwd": repo.canonical_path(),
        "hook_event_name": hook_event_name,
        "tool_name": "Write",
        "tool_use_id": format!("toolu_{external_session_id}"),
        "session_id": external_session_id,
        "transcript_path": transcript_path,
        "tool_input": {
            "file_path": file_path,
        },
    })
    .to_string();

    repo.checkpoint_with_hook_input("claude", &hook_input)
        .expect("Claude checkpoint should succeed");
}

fn prepare_edge_recovery(
    repo: &TestRepo,
    file_name: &str,
    transcript_path: &Path,
    external_session_id: &str,
) {
    let file_path = repo.path().join(file_name);

    for (before, after) in [
        ("base\nai before\n", "base\nai before edited\n"),
        (
            "base\nai before edited\nai after\n",
            "base\nai before edited\nai after edited\n",
        ),
    ] {
        fs::write(&file_path, before).unwrap();
        claude_edge_file_edit_checkpoint(
            repo,
            &file_path,
            transcript_path,
            "PreToolUse",
            external_session_id,
        );
        fs::write(&file_path, after).unwrap();
        claude_edge_file_edit_checkpoint(
            repo,
            &file_path,
            transcript_path,
            "PostToolUse",
            external_session_id,
        );
    }

    fs::write(
        &file_path,
        "base\nai before edited\nunknown gap\nai after edited\n",
    )
    .unwrap();
}

fn wait_for_edge_recovery_metric(db_path: &str, file_path: &str) -> MetricEvent {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let db = MetricsDatabase::open_at_path(Path::new(db_path))
            .expect("metrics db should open at isolated path");
        let records = db
            .get_metric_history(0, None, &[MetricEventId::Checkpoint as u16])
            .expect("checkpoint metric history should load");
        if let Some(record) = records.into_iter().find(|record| {
            sparse_str(&record.event.values, checkpoint_pos::CHECKPOINT_TYPE)
                == Some("recovered_edge_extension")
                && sparse_str(&record.event.values, checkpoint_pos::FILE_PATH) == Some(file_path)
        }) {
            return record.event;
        }

        if Instant::now() >= deadline {
            panic!("recovered_edge_extension checkpoint metric for {file_path} was not persisted");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn claude_model_transcript() -> tempfile::NamedTempFile {
    let transcript = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .expect("Claude transcript tempfile should be created");
    fs::write(
        transcript.path(),
        r#"{"message":{"role":"assistant","model":"claude-sonnet-4"}}
"#,
    )
    .unwrap();
    transcript
}

struct EdgeRecoveryMetricFixture {
    repo: TestRepo,
    _transcript: tempfile::NamedTempFile,
    _metrics_db_dir: tempfile::TempDir,
    metrics_db_path: String,
}

fn edge_recovery_metric_fixture(
    file_name: &str,
    external_session_id: &str,
) -> EdgeRecoveryMetricFixture {
    let (metrics_db_dir, metrics_db_path) = isolated_metrics_db_path();
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_METRICS_DB_PATH", metrics_db_path.as_str())]);
    let transcript = claude_model_transcript();

    fs::write(repo.path().join(file_name), "base\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut file = repo.filename(file_name);
    file.assert_committed_lines(lines!["base".unattributed_human()]);
    prepare_edge_recovery(&repo, file_name, transcript.path(), external_session_id);

    EdgeRecoveryMetricFixture {
        repo,
        _transcript: transcript,
        _metrics_db_dir: metrics_db_dir,
        metrics_db_path,
    }
}

fn assert_edge_recovery_attribution(repo: &TestRepo, file_name: &str) {
    let mut file = repo.filename(file_name);
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "ai before edited".ai(),
        "unknown gap".ai(),
        "ai after edited".ai(),
    ]);
}

mod checkpoint_persistence;
mod dirty_state;
mod edge_recovery;
mod history_recovery;
