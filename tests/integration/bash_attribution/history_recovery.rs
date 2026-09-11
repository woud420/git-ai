use super::*;

#[test]
fn test_bash_recovery_uses_commit_time_file_timestamps_when_processing_is_delayed() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [
        ("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str()),
        (
            "GIT_AI_TEST_DELAY_SIDE_EFFECT_MS_FOR_COMMAND",
            "remote=6500",
        ),
    ];
    let repo = TestRepo::new_with_daemon_env(&env);
    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("delayed.txt");

    fs::write(&file_path, "dirty pre-bash line\n").unwrap();

    let simple_fixture = fixture_path("codex-session-simple.jsonl");
    let transcript_path = repo_root.join("codex-transcript.jsonl");
    fs::copy(&simple_fixture, &transcript_path).unwrap();

    let pre_hook_input = json!({
        "session_id": "delayed-commit-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "delayed-commit-tool",
        "tool_input": { "command": "printf 'ai bash line\\n' >> delayed.txt" },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();
    repo.checkpoint_with_hook_input("codex", &pre_hook_input)
        .expect("codex pre-hook checkpoint should succeed");

    fs::write(&file_path, "dirty pre-bash line\nai bash line\n").unwrap();

    let post_hook_input = json!({
        "session_id": "delayed-commit-session",
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_use_id": "delayed-commit-tool",
        "tool_input": { "command": "printf 'ai bash line\\n' >> delayed.txt" },
        "transcript_path": transcript_path.to_string_lossy().to_string()
    })
    .to_string();
    repo.checkpoint_with_hook_input("codex", &post_hook_input)
        .expect("codex post-hook checkpoint should succeed");

    repo.git_without_test_sync_for_test(
        &[
            "remote",
            "add",
            "delayed-side-effect",
            "https://example.invalid/repo.git",
        ],
        &[],
    )
    .expect("remote add should succeed");
    repo.git_without_test_sync_for_test(&["add", "-A"], &[])
        .expect("add should succeed");
    repo.git_without_test_sync_for_test(&["commit", "-m", "Delayed commit"], &[])
        .expect("commit should succeed");

    thread::sleep(Duration::from_millis(3500));
    fs::write(
        &file_path,
        "dirty pre-bash line\nai bash line\nmanual edit after commit\n",
    )
    .unwrap();

    let mut file = repo.filename("delayed.txt");
    file.assert_committed_lines(lines!["dirty pre-bash line".ai(), "ai bash line".ai(),]);
}

#[test]
fn test_bash_recovery_does_not_attribute_manual_edit_after_unrelated_bash() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str())];
    let repo = TestRepo::new_with_daemon_env(&env);
    let repo_root = repo.canonical_path();
    set_daemon_socket_for_test(repo.daemon_control_socket_path());

    repo.git(&["commit", "--allow-empty", "-m", "initial"])
        .expect("initial commit should succeed");

    let agent = AgentId {
        tool: "codex".to_string(),
        id: "manual-after-bash-session".to_string(),
        model: "gpt-5".to_string(),
    };

    handle_bash_pre_tool_use_with_context(
        &repo_root,
        "manual-after-bash-session",
        "manual-after-bash-tool-1",
        &agent,
        None,
        "t_manualpre000",
        Some("true"),
    )
    .expect("pre bash hook should record durable start");

    let post_result = handle_bash_post_tool_use(
        &repo_root,
        "manual-after-bash-session",
        "manual-after-bash-tool-1",
        &agent,
        None,
        "t_manualpost00",
        Some("true"),
    )
    .expect("post bash hook should record durable end");
    assert!(
        matches!(post_result.action, BashCheckpointAction::NoChanges),
        "bash call should not emit a normal checkpoint"
    );

    fs::write(repo_root.join("manual-after.txt"), "manual after bash\n").unwrap();
    repo.stage_all_and_commit("Manual edit after bash").unwrap();

    let mut file = repo.filename("manual-after.txt");
    file.assert_committed_lines(lines!["manual after bash".unattributed_human()]);
}

#[test]
fn test_bash_recovery_does_not_attribute_manual_edit_before_unrelated_bash() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str())];
    let repo = TestRepo::new_with_daemon_env(&env);
    let repo_root = repo.canonical_path();
    set_daemon_socket_for_test(repo.daemon_control_socket_path());

    repo.git(&["commit", "--allow-empty", "-m", "initial"])
        .expect("initial commit should succeed");

    fs::write(repo_root.join("manual-before.txt"), "manual before bash\n").unwrap();

    let agent = AgentId {
        tool: "codex".to_string(),
        id: "manual-before-bash-session".to_string(),
        model: "gpt-5".to_string(),
    };

    handle_bash_pre_tool_use_with_context(
        &repo_root,
        "manual-before-bash-session",
        "manual-before-bash-tool-1",
        &agent,
        None,
        "t_manualbefpre",
        Some("true"),
    )
    .expect("pre bash hook should record durable start");

    let post_result = handle_bash_post_tool_use(
        &repo_root,
        "manual-before-bash-session",
        "manual-before-bash-tool-1",
        &agent,
        None,
        "t_manualbefpst",
        Some("true"),
    )
    .expect("post bash hook should record durable end");
    assert!(
        matches!(post_result.action, BashCheckpointAction::NoChanges),
        "bash call should not emit a normal checkpoint"
    );

    repo.stage_all_and_commit("Manual edit before bash")
        .unwrap();

    let mut file = repo.filename("manual-before.txt");
    file.assert_committed_lines(lines!["manual before bash".unattributed_human()]);
}

#[test]
fn test_bash_history_recovers_untracked_lines_when_post_snapshot_fails() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str())];
    let repo = TestRepo::new_with_daemon_env(&env);
    let repo_root = repo.canonical_path();
    set_daemon_socket_for_test(repo.daemon_control_socket_path());

    let initial_path = repo_root.join("base.txt");
    fs::write(&initial_path, "base\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let agent = AgentId {
        tool: "codex".to_string(),
        id: "recover-bash-session".to_string(),
        model: "gpt-5".to_string(),
    };

    handle_bash_pre_tool_use_with_context(
        &repo_root,
        "recover-bash-session",
        "recover-tool-1",
        &agent,
        None,
        "t_recoverpre000",
        Some("printf recovered > recovered.txt"),
    )
    .expect("pre bash hook should record durable start");

    let recovered_path = repo_root.join("recovered.txt");
    fs::write(&recovered_path, "recovered by bash\n").unwrap();

    set_walk_timeout_ms_for_test(0);
    let post_result = handle_bash_post_tool_use(
        &repo_root,
        "recover-bash-session",
        "recover-tool-1",
        &agent,
        None,
        "t_recoverpost00",
        Some("printf recovered > recovered.txt"),
    )
    .expect("post bash hook should degrade gracefully");
    reset_timeout_overrides_for_test();
    assert!(
        matches!(post_result.action, BashCheckpointAction::SnapshotFailed),
        "post hook should not emit a normal checkpoint in this regression setup"
    );

    repo.stage_all_and_commit("Recover bash attribution")
        .unwrap();

    let mut recovered = repo.filename("recovered.txt");
    recovered.assert_committed_lines(lines!["recovered by bash".ai()]);
}

#[test]
fn test_bash_history_recovers_when_bash_checkpoint_was_recorded_elsewhere() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str())];
    let source_repo = TestRepo::new_with_daemon_env(&env);
    let target_repo = TestRepo::new_with_daemon_env(&env);

    let source_root = source_repo.canonical_path();
    let target_root = target_repo.canonical_path();
    set_daemon_socket_for_test(source_repo.daemon_control_socket_path());

    let target_file = target_root.join("elsewhere.txt");
    fs::write(&target_file, "base\n").unwrap();
    target_repo
        .stage_all_and_commit("Initial target commit")
        .unwrap();

    let mut target = target_repo.filename("elsewhere.txt");
    target.assert_committed_lines(lines!["base".unattributed_human()]);

    let agent = AgentId {
        tool: "codex".to_string(),
        id: "cross-repo-bash-session".to_string(),
        model: "gpt-5".to_string(),
    };
    let command = format!("printf 'from elsewhere\\n' >> {}", target_file.display());

    handle_bash_pre_tool_use_with_context(
        &source_root,
        "cross-repo-bash-session",
        "cross-repo-tool-1",
        &agent,
        None,
        "t_crosspre000",
        Some(&command),
    )
    .expect("pre bash hook should record durable start from source repo");

    fs::write(&target_file, "base\nfrom elsewhere\n").unwrap();

    set_walk_timeout_ms_for_test(0);
    let post_result = handle_bash_post_tool_use(
        &source_root,
        "cross-repo-bash-session",
        "cross-repo-tool-1",
        &agent,
        None,
        "t_crosspost00",
        Some(&command),
    )
    .expect("post bash hook should degrade gracefully");
    reset_timeout_overrides_for_test();
    assert!(
        matches!(post_result.action, BashCheckpointAction::SnapshotFailed),
        "post hook should not emit a normal checkpoint in this regression setup"
    );

    target_repo
        .stage_all_and_commit("Recover cross-repo bash attribution")
        .unwrap();
    target.assert_committed_lines(lines!["base".unattributed_human(), "from elsewhere".ai()]);
}

#[test]
fn test_bash_history_recovers_dirty_lines_present_before_bash() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str())];
    let repo = TestRepo::new_with_daemon_env(&env);
    let repo_root = repo.canonical_path();
    set_daemon_socket_for_test(repo.daemon_control_socket_path());

    let file_path = repo_root.join("mixed.txt");
    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    fs::write(&file_path, "base\ndirty before bash\n").unwrap();
    repo.git_ai(&["checkpoint", "human", "mixed.txt"])
        .expect("legacy pre-bash checkpoint should record dirty untracked content");

    let agent = AgentId {
        tool: "codex".to_string(),
        id: "recover-mixed-bash-session".to_string(),
        model: "gpt-5".to_string(),
    };

    handle_bash_pre_tool_use_with_context(
        &repo_root,
        "recover-mixed-bash-session",
        "recover-mixed-tool-1",
        &agent,
        None,
        "t_mixedpre0000",
        Some("printf recovered >> mixed.txt"),
    )
    .expect("pre bash hook should record durable start");

    fs::write(&file_path, "base\ndirty before bash\nbash recovered line\n").unwrap();

    set_walk_timeout_ms_for_test(0);
    let post_result = handle_bash_post_tool_use(
        &repo_root,
        "recover-mixed-bash-session",
        "recover-mixed-tool-1",
        &agent,
        None,
        "t_mixedpost000",
        Some("printf recovered >> mixed.txt"),
    )
    .expect("post bash hook should degrade gracefully");
    reset_timeout_overrides_for_test();
    assert!(
        matches!(post_result.action, BashCheckpointAction::SnapshotFailed),
        "post hook should not emit a normal checkpoint in this regression setup"
    );

    repo.stage_all_and_commit("Recover dirty and bash lines")
        .unwrap();

    let mut file = repo.filename("mixed.txt");
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "dirty before bash".ai(),
        "bash recovered line".ai(),
    ]);
}

#[test]
fn test_bash_history_recovers_shifted_dirty_lines_present_before_bash() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let env = [("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH", bash_db_path.as_str())];
    let repo = TestRepo::new_with_daemon_env(&env);
    let repo_root = repo.canonical_path();
    set_daemon_socket_for_test(repo.daemon_control_socket_path());

    let file_path = repo_root.join("shifted.txt");
    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    fs::write(&file_path, "base\ndirty before bash\n").unwrap();
    repo.git_ai(&["checkpoint", "human", "shifted.txt"])
        .expect("legacy pre-bash checkpoint should record dirty untracked content");

    let agent = AgentId {
        tool: "codex".to_string(),
        id: "recover-shifted-bash-session".to_string(),
        model: "gpt-5".to_string(),
    };

    handle_bash_pre_tool_use_with_context(
        &repo_root,
        "recover-shifted-bash-session",
        "recover-shifted-tool-1",
        &agent,
        None,
        "t_shiftpre000",
        Some("python - <<'PY'\nfrom pathlib import Path\np = Path('shifted.txt')\np.write_text('bash recovered line\\n' + p.read_text())\nPY"),
    )
    .expect("pre bash hook should record durable start");

    fs::write(&file_path, "bash recovered line\nbase\ndirty before bash\n").unwrap();

    set_walk_timeout_ms_for_test(0);
    let post_result = handle_bash_post_tool_use(
        &repo_root,
        "recover-shifted-bash-session",
        "recover-shifted-tool-1",
        &agent,
        None,
        "t_shiftpost00",
        Some("python - <<'PY'\nfrom pathlib import Path\np = Path('shifted.txt')\np.write_text('bash recovered line\\n' + p.read_text())\nPY"),
    )
    .expect("post bash hook should degrade gracefully");
    reset_timeout_overrides_for_test();
    assert!(
        matches!(post_result.action, BashCheckpointAction::SnapshotFailed),
        "post hook should not emit a normal checkpoint in this regression setup"
    );

    repo.stage_all_and_commit("Recover shifted dirty and bash lines")
        .unwrap();

    let mut file = repo.filename("shifted.txt");
    file.assert_committed_lines(lines![
        "bash recovered line".ai(),
        "base".unattributed_human(),
        "dirty before bash".ai(),
    ]);
}
