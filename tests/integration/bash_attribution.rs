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

mod history_recovery;

#[test]
fn test_bash_checkpoints_v2_records_for_recovery_without_working_log_checkpoints() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str())];
    let mut repo = TestRepo::new_with_daemon_env(&env);
    repo.patch_git_ai_config(|patch| {
        patch.feature_flags = Some(json!({"bash_checkpoints_v2": true}));
    });
    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("example.txt");

    fs::write(&file_path, "original line\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut file = repo.filename("example.txt");
    file.assert_committed_lines(lines!["original line".unattributed_human()]);

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-transcript.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::pre_bash(
            "bash-v2-session",
            &repo_root,
            "bash-v2-tool",
            "printf 'written by bash\\n' >> example.txt",
        )
        .with_transcript_path(&transcript_path),
    );

    fs::write(&file_path, "original line\nwritten by bash\n").unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::post_bash(
            "bash-v2-session",
            &repo_root,
            "bash-v2-tool",
            "printf 'written by bash\\n' >> example.txt",
        )
        .with_transcript_path(&transcript_path),
    );

    let checkpoints = repo.current_working_logs().read_all_checkpoints().unwrap();
    assert!(
        checkpoints.is_empty(),
        "bash checkpoints v2 should only record recovery metadata, not normal checkpoints"
    );

    repo.stage_all_and_commit("After bash v2").unwrap();
    file.assert_committed_lines(lines![
        "original line".unattributed_human(),
        "written by bash".ai(),
    ]);

    let db = BashHistoryDatabase::open_at_path(std::path::Path::new(&bash_db_path)).unwrap();
    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].session_id, "bash-v2-session");
    assert_eq!(calls[0].tool_use_id, "bash-v2-tool");
    assert_eq!(
        calls[0].repo_work_dir.as_deref(),
        Some(repo_root.to_string_lossy().as_ref())
    );
    assert!(calls[0].start_trace_id.is_some());
    assert!(calls[0].end_trace_id.is_some());
}

#[test]
fn test_bash_checkpoints_v2_denies_before_attempt_persistence() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str())];
    let mut repo = TestRepo::new_with_daemon_env_and_patch(&env, |patch| {
        patch.feature_flags = Some(json!({
            "bash_checkpoints_v2": true,
            "checkpoint_debug_log": true
        }));
    });
    let malformed = repo.path().join("malformed");
    fs::create_dir_all(&malformed).unwrap();
    fs::write(malformed.join(".git"), "not a gitdir pointer\n").unwrap();
    let hook_input = json!({
        "session_id": "malformed-bash-session",
        "cwd": malformed.to_string_lossy(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "malformed-bash-tool",
        "tool_input": { "command": "printf sensitive >> private.txt" }
    })
    .to_string();

    let output = repo
        .checkpoint_with_hook_input("codex", &hook_input)
        .expect("an authorization denial should preserve the hook exit-zero contract");

    assert!(output.contains("repository authorization could not be verified"));
    let db = BashHistoryDatabase::open_at_path(std::path::Path::new(&bash_db_path)).unwrap();
    assert!(
        db.all_calls_for_test().unwrap().is_empty(),
        "a malformed-repository bash hook must not persist an attempt"
    );

    repo.patch_git_ai_config(|patch| {
        patch.allowed_repositories = Some(Vec::new());
    });
    let hook_input = json!({
        "session_id": "denied-bash-session",
        "cwd": repo.canonical_path().to_string_lossy(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "denied-bash-tool",
        "tool_input": { "command": "printf sensitive >> private.txt" }
    })
    .to_string();
    let output = repo
        .checkpoint_with_hook_input("codex", &hook_input)
        .expect("an authorization denial should preserve the hook exit-zero contract");

    assert!(output.contains("no repositories are allowed"));
    let db = BashHistoryDatabase::open_at_path(std::path::Path::new(&bash_db_path)).unwrap();
    assert!(
        db.all_calls_for_test().unwrap().is_empty(),
        "an empty-allowlist bash hook must not persist an attempt"
    );
    assert!(
        !repo
            .test_home_path()
            .join(".git-ai/internal/checkpoint-debug-logs")
            .exists(),
        "a malformed-repository bash hook must not persist its raw hook input"
    );
}

#[test]
fn test_codex_parent_cwd_bash_attempt_is_denied_before_persistence() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str())];
    let repo = TestRepo::new_with_daemon_env_and_patch(&env, |patch| {
        patch.feature_flags = Some(json!({"checkpoint_debug_log": true}));
    });
    let repo_root = repo.canonical_path();
    let parent_cwd = repo_root.parent().unwrap().to_path_buf();
    let repo_name = repo_root.file_name().unwrap().to_string_lossy().to_string();

    fs::write(repo_root.join("README.md"), "base\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    fs::create_dir_all(repo_root.join("src")).unwrap();
    let command = format!("cd {repo_name} && printf x >> src/parent-cwd.txt");
    let pre_hook_input = json!({
        "session_id": "parent-cwd-session",
        "cwd": parent_cwd.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "parent-cwd-tool",
        "tool_input": { "command": command },
        "model": "gpt-5"
    })
    .to_string();

    let pre_output = repo
        .git_ai_from_working_dir(
            &parent_cwd,
            &["checkpoint", "codex", "--hook-input", &pre_hook_input],
        )
        .expect("parent-cwd authorization denial should preserve hook exit zero");
    assert!(pre_output.contains("repository authorization could not be verified"));

    fs::write(repo_root.join("src/parent-cwd.txt"), "x\n").unwrap();

    let post_hook_input = json!({
        "session_id": "parent-cwd-session",
        "cwd": parent_cwd.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "parent-cwd-tool",
        "tool_input": { "command": command },
        "model": "gpt-5"
    })
    .to_string();

    let post_output = repo
        .git_ai_from_working_dir(
            &parent_cwd,
            &["checkpoint", "codex", "--hook-input", &post_hook_input],
        )
        .expect("parent-cwd authorization denial should preserve hook exit zero");
    assert!(post_output.contains("repository authorization could not be verified"));

    let commit = repo
        .stage_all_and_commit("Parent cwd bash write")
        .expect("commit should succeed");

    let mut file = repo.filename("src/parent-cwd.txt");
    file.assert_committed_lines(lines!["x".unattributed_human()]);
    assert!(
        commit.authorship_log.metadata.sessions.is_empty(),
        "a denied parent-cwd hook must not create false AI session attribution"
    );

    let db = BashHistoryDatabase::open_at_path(std::path::Path::new(&bash_db_path)).unwrap();
    assert!(
        db.all_calls_for_test().unwrap().is_empty(),
        "a denied parent-cwd hook must not persist BashHookAttempt metadata"
    );
    assert!(
        !repo
            .test_home_path()
            .join(".git-ai/internal/checkpoint-debug-logs")
            .exists(),
        "a denied parent-cwd hook must not persist raw debug input"
    );
}

#[test]
fn test_bash_pre_legacy_checkpoint_recovers_dirty_edge_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("example.txt");

    let initial = "original line\n";
    fs::write(&file_path, initial).unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut file = repo.filename("example.txt");
    file.assert_committed_lines(lines!["original line".unattributed_human()]);

    let after_dirty_edit = "original line\ndirty pre-bash line\n";
    fs::write(&file_path, after_dirty_edit).unwrap();
    repo.git_ai(&["checkpoint", "human", "example.txt"])
        .unwrap();

    let after_bash = "original line\ndirty pre-bash line\nai bash line\n";
    fs::write(&file_path, after_bash).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();

    repo.stage_all_and_commit("After bash").unwrap();
    file.assert_committed_lines(lines![
        "original line".unattributed_human(),
        "dirty pre-bash line".ai(),
        "ai bash line".ai(),
    ]);
}

#[test]
fn test_bash_clean_files_only_bash_changes_get_ai_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("clean.txt");

    let initial = "committed line\n";
    fs::write(&file_path, initial).unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut file = repo.filename("clean.txt");
    file.assert_committed_lines(lines!["committed line".unattributed_human()]);

    repo.git_ai(&["checkpoint", "human", "clean.txt"]).unwrap();

    let after_bash = "committed line\nbash added this\n";
    fs::write(&file_path, after_bash).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "clean.txt"])
        .unwrap();

    repo.stage_all_and_commit("After bash").unwrap();
    file.assert_committed_lines(lines![
        "committed line".unattributed_human(),
        "bash added this".ai(),
    ]);
}

#[test]
fn test_bash_multiple_files_mixed_dirty_state() {
    let repo = TestRepo::new();
    let a_path = repo.path().join("a.txt");
    let b_path = repo.path().join("b.txt");

    fs::write(&a_path, "line a\n").unwrap();
    fs::write(&b_path, "line b\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut file_a = repo.filename("a.txt");
    let mut file_b = repo.filename("b.txt");
    file_a.assert_committed_lines(lines!["line a".unattributed_human()]);
    file_b.assert_committed_lines(lines!["line b".unattributed_human()]);

    fs::write(&a_path, "line a\ndirty touched a\n").unwrap();

    repo.git_ai(&["checkpoint", "human", "a.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "human", "b.txt"]).unwrap();

    fs::write(&a_path, "line a\ndirty touched a\nbash touched a\n").unwrap();
    fs::write(&b_path, "line b\nbash touched b\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "a.txt"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "b.txt"]).unwrap();

    repo.stage_all_and_commit("After bash").unwrap();
    file_a.assert_committed_lines(lines![
        "line a".unattributed_human(),
        "dirty touched a".ai(),
        "bash touched a".ai(),
    ]);
    file_b.assert_committed_lines(lines!["line b".unattributed_human(), "bash touched b".ai()]);
}

/// Orchestrator-level regression test: fires through the real codex
/// preset/orchestrator path (not manual `git-ai checkpoint human` CLI
/// calls). The bash history recovery pass intentionally minimizes untracked
/// lines, so pre-bash dirty content committed with the bash result is recovered
/// as AI when the bash invocation is the nearest candidate.
#[test]
fn test_codex_preset_bash_recovery_minimizes_dirty_untracked_attribution() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str())];
    let repo = TestRepo::new_with_daemon_env(&env);
    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("example.txt");

    fs::write(&file_path, "original line\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut file = repo.filename("example.txt");
    file.assert_committed_lines(lines!["original line".unattributed_human()]);

    // Dirty untracked content exists before the AI bash tool runs.
    fs::write(&file_path, "original line\ndirty pre-bash line\n").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-transcript.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::pre_bash("attr-pre-sess", &repo_root, "attr-bash-1", "echo hello")
            .with_transcript_path(&transcript_path),
    );

    // AI bash tool edits the file.
    fs::write(
        &file_path,
        "original line\ndirty pre-bash line\nai bash line\n",
    )
    .unwrap();

    checkpoint_codex(
        &repo,
        CodexHookInput::post_bash("attr-pre-sess", &repo_root, "attr-bash-1", "echo hello")
            .with_transcript_path(&transcript_path),
    );

    repo.stage_all_and_commit("After codex bash").unwrap();
    file.assert_committed_lines(lines![
        "original line".unattributed_human(),
        "dirty pre-bash line".ai(),
        "ai bash line".ai(),
    ]);
}

#[test]
fn test_edge_extension_recovers_unknown_gap_between_ai_attributions() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("edge.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    fs::write(&file_path, "base\nai before\nunknown gap\nai after\n").unwrap();
    repo.git_ai(&["checkpoint", "human", "edge.txt"])
        .expect("legacy human checkpoint should mark current content untracked");

    fs::write(
        &file_path,
        "base\nai before edited\nunknown gap\nai after edited\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "edge.txt"])
        .expect("AI checkpoint should only directly attribute the edited lines");

    repo.stage_all_and_commit("Recover edge attribution")
        .unwrap();

    let mut file = repo.filename("edge.txt");
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "ai before edited".ai(),
        "unknown gap".ai(),
        "ai after edited".ai(),
    ]);
}

#[test]
fn test_edge_extension_recovers_leading_and_trailing_unknown_lines_near_ai_block() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("edge-fringes.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    fs::write(
        &file_path,
        "\
base
leading dirty
ai one placeholder
ai two placeholder
ai three placeholder
trailing dirty 1
trailing dirty 2
trailing dirty 3
trailing dirty 4
",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "human", "edge-fringes.txt"])
        .expect("legacy human checkpoint should mark current content untracked");

    fs::write(
        &file_path,
        "\
base
leading dirty
ai one
ai two
ai three
trailing dirty 1
trailing dirty 2
trailing dirty 3
trailing dirty 4
",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "edge-fringes.txt"])
        .expect("AI checkpoint should only directly attribute the edited block");

    repo.stage_all_and_commit("Recover edge fringes").unwrap();

    let mut file = repo.filename("edge-fringes.txt");
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "leading dirty".ai(),
        "ai one".ai(),
        "ai two".ai(),
        "ai three".ai(),
        "trailing dirty 1".ai(),
        "trailing dirty 2".ai(),
        "trailing dirty 3".ai(),
        "trailing dirty 4".unattributed_human(),
    ]);
}

// Regression coverage for ENG-315 and upstream git-ai-project/git-ai#2214.
#[test]
fn test_edge_extension_recovery_metric_preserves_known_identity() {
    const FILE_NAME: &str = "known-edge.txt";
    const SESSION_ID: &str = "known-edge-session";

    let fixture = edge_recovery_metric_fixture(FILE_NAME, SESSION_ID);
    fixture
        .repo
        .stage_all_and_commit("Recover edge attribution")
        .unwrap();
    assert_edge_recovery_attribution(&fixture.repo, FILE_NAME);

    let event = wait_for_edge_recovery_metric(&fixture.metrics_db_path, FILE_NAME);
    assert_eq!(sparse_str(&event.attrs, attr_pos::TOOL), Some("claude"));
    assert_eq!(
        sparse_str(&event.attrs, attr_pos::MODEL),
        Some("claude-sonnet-4")
    );
    assert_eq!(
        sparse_str(&event.attrs, attr_pos::EXTERNAL_SESSION_ID),
        Some(SESSION_ID)
    );
}

#[test]
fn test_edge_extension_recovery_metric_keeps_unknown_identity_sparse() {
    const FILE_NAME: &str = "unknown-edge.txt";
    const SESSION_ID: &str = "unknown-edge-session";

    let fixture = edge_recovery_metric_fixture(FILE_NAME, SESSION_ID);

    let working_log = fixture.repo.current_working_logs();
    let mut cleared_agent_ids = 0;
    let checkpoints = working_log
        .mutate_all_checkpoints(|checkpoints| {
            for checkpoint in checkpoints {
                if checkpoint
                    .agent_id
                    .as_ref()
                    .is_some_and(|agent_id| agent_id.id == SESSION_ID)
                {
                    checkpoint.agent_id = None;
                    cleared_agent_ids += 1;
                }
            }
            Ok(())
        })
        .unwrap();
    assert!(
        cleared_agent_ids > 0,
        "fixture should remove the unknown session's existing identity"
    );
    assert!(checkpoints.iter().all(|checkpoint| {
        checkpoint
            .agent_id
            .as_ref()
            .is_none_or(|agent_id| agent_id.id != SESSION_ID)
    }));

    fixture
        .repo
        .stage_all_and_commit("Recover edge attribution")
        .unwrap();
    let mut file = fixture.repo.filename(FILE_NAME);
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "ai before edited".unattributed_human(),
        "unknown gap".unattributed_human(),
        "ai after edited".unattributed_human(),
    ]);

    let unknown_event = wait_for_edge_recovery_metric(&fixture.metrics_db_path, FILE_NAME);
    for pos in [
        attr_pos::TOOL,
        attr_pos::MODEL,
        attr_pos::EXTERNAL_SESSION_ID,
    ] {
        assert!(
            !unknown_event.attrs.contains_key(&pos.to_string()),
            "unknown identity field at position {pos} should stay sparse"
        );
    }
}
