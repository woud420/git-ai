use super::{
    BashHistoryDatabase, CodexHookInput, ExpectedLineExt, TestRepo, checkpoint_codex, fixture_path,
    fs, isolated_bash_history_db_path, json,
};

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
