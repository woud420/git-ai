use super::{
    CodexHookInput, ExpectedLineExt, TestRepo, checkpoint_codex, fixture_path, fs,
    isolated_bash_history_db_path,
};

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
