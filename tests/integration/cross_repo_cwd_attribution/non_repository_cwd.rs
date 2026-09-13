use super::{ExpectedLineExt, TestRepo, create_unique_workspace, fixture_path, fs, json};

// ===========================================================================
// Issue #954 regression: CWD is a completely unrelated non-git directory
//
// When Claude (or any agent) is launched from a directory that is not inside
// any git repository (e.g. /tmp, ~/), all AI attribution was dropped because
// the workspace boundary check in find_repository_for_file rejected files
// that were not children of the CWD directory.
// ===========================================================================

/// Regression test for issue #954:
/// When CWD is a non-git directory completely unrelated to the target repo
/// (not a parent/ancestor of the target), checkpoint must still write to the
/// target repo's working logs so that the commit carries AI attribution.
#[test]
fn test_issue_954_non_git_cwd_unrelated_to_target_repo_mock_ai() {
    // Create a non-git workspace directory (simulates launching from /tmp or ~/)
    // This directory is a SIBLING of the target repo, not its parent.
    let cwd_workspace = create_unique_workspace("git-ai-954-non-git-cwd");

    // Target repo lives in a completely separate location (also under /tmp but as a sibling)
    let repo_target = TestRepo::new();

    // Set up initial commit in target repo
    let mut readme = repo_target.filename("README.md");
    readme.set_contents(crate::lines!["# Target Repo"]);
    repo_target.stage_all_and_commit("initial commit").unwrap();

    // Write AI content to the target repo
    fs::write(
        repo_target.path().join("auth.ts"),
        "export function verifyJwt(token: string): boolean {\n  return true;\n}\n",
    )
    .unwrap();

    let target_file = repo_target.canonical_path().join("auth.ts");

    // Checkpoint from the unrelated non-git CWD with an absolute path to the file.
    // Before the fix, this failed because the workspace boundary was set to cwd_workspace,
    // and find_repository_for_file refused to search outside that boundary.
    repo_target
        .git_ai_from_working_dir(
            &cwd_workspace,
            &["checkpoint", "mock_ai", target_file.to_str().unwrap()],
        )
        .expect("checkpoint from non-git CWD unrelated to target repo should succeed");

    // Verify the working log was written in the target repo before committing
    let working_log = repo_target.current_working_logs();
    let ai_files = working_log.all_ai_touched_files().unwrap_or_default();
    assert!(
        !ai_files.is_empty(),
        "Issue #954: Working log entries should exist in the target repo \
         when checkpoint is run from a non-git CWD that is unrelated to the target repo. \
         Found no AI-touched files in working log."
    );

    // Commit in the target repo and verify AI attribution
    let commit = repo_target.stage_all_and_commit("add AI jwt auth").unwrap();
    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "Issue #954: AI attribution should be present when Claude is launched from \
         a non-git directory (e.g. /tmp) and writes files to a separate git repo. \
         Expected non-empty attestations but got none."
    );

    let _ = fs::remove_dir_all(&cwd_workspace);
}

/// Issue #954 variant: non-git CWD, multiple files in multiple separate repos.
/// Verifies that all target repos receive correct attribution.
#[test]
fn test_issue_954_non_git_cwd_multiple_target_repos() {
    let cwd_workspace = create_unique_workspace("git-ai-954-multi-target-cwd");

    let repo_a = TestRepo::new();
    let repo_b = TestRepo::new();

    for (repo, name) in [(&repo_a, "A"), (&repo_b, "B")] {
        let mut readme = repo.filename("README.md");
        readme.set_contents(crate::lines![format!("# Repo {}", name)]);
        repo.stage_all_and_commit("initial commit").unwrap();
    }

    fs::write(repo_a.path().join("feature_a.ts"), "AI code A\n").unwrap();
    fs::write(repo_b.path().join("feature_b.ts"), "AI code B\n").unwrap();

    let file_a = repo_a.canonical_path().join("feature_a.ts");
    let file_b = repo_b.canonical_path().join("feature_b.ts");

    repo_a
        .git_ai_from_working_dir(
            &cwd_workspace,
            &[
                "checkpoint",
                "mock_ai",
                file_a.to_str().unwrap(),
                file_b.to_str().unwrap(),
            ],
        )
        .expect("multi-repo checkpoint from non-git CWD should succeed");

    let commit_a = repo_a.stage_all_and_commit("AI edits in A").unwrap();
    assert!(
        !commit_a.authorship_log.attestations.is_empty(),
        "Issue #954 (multi): repo_a should have AI attestations when checkpoint \
         runs from a non-git CWD."
    );

    let commit_b = repo_b.stage_all_and_commit("AI edits in B").unwrap();
    assert!(
        !commit_b.authorship_log.attestations.is_empty(),
        "Issue #954 (multi): repo_b should have AI attestations when checkpoint \
         runs from a non-git CWD."
    );

    let _ = fs::remove_dir_all(&cwd_workspace);
}

/// Issue #954 variant: Claude preset with non-git CWD and file in a separate repo.
/// Verifies that the Claude PostToolUse checkpoint records prompts correctly.
#[test]
fn test_issue_954_claude_preset_non_git_cwd() {
    let cwd_workspace = create_unique_workspace("git-ai-954-claude-cwd");
    let mut repo_target = TestRepo::new();

    // Enable prompt sharing so we can assert prompts are non-empty
    repo_target.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let target_root = repo_target.canonical_path();

    // Set up initial commit
    let target_file_path = target_root.join("auth.ts");
    fs::write(&target_file_path, "// initial\n").unwrap();
    repo_target.stage_all_and_commit("initial commit").unwrap();

    // Copy transcript fixture
    let transcript_path = target_root.join("claude-session.jsonl");
    let fixture = fixture_path("example-claude-code.jsonl");
    fs::copy(&fixture, &transcript_path).unwrap();

    // Build hook input: cwd is the non-git workspace, file is in repo_target
    let hook_input = json!({
        "cwd": cwd_workspace.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "transcript_path": transcript_path.to_string_lossy().to_string(),
        "tool_input": {
            "file_path": target_file_path.to_string_lossy().to_string()
        }
    })
    .to_string();

    // Simulate AI edit
    fs::write(
        &target_file_path,
        "// initial\nexport function verifyJwt(): boolean { return true; }\n",
    )
    .unwrap();

    // Run checkpoint from non-git CWD with Claude preset
    repo_target
        .git_ai_from_working_dir(
            &cwd_workspace,
            &["checkpoint", "claude", "--hook-input", &hook_input],
        )
        .expect("Claude checkpoint from non-git CWD should succeed");

    // Verify the working log was written in the target repo
    let working_log = repo_target.current_working_logs();
    let ai_files = working_log.all_ai_touched_files().unwrap_or_default();
    assert!(
        !ai_files.is_empty(),
        "Issue #954 (claude): Working log entries should exist in target repo \
         when Claude PostToolUse fires from a non-git CWD."
    );

    let commit = repo_target.stage_all_and_commit("AI auth changes").unwrap();
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Issue #954 (claude): Sessions should not be empty in target repo's git note \
         when Claude is launched from a non-git CWD."
    );
    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "Issue #954 (claude): AI attestations should be present when Claude is launched \
         from a non-git CWD."
    );

    let _ = fs::remove_dir_all(&cwd_workspace);
}

// ===========================================================================
// Scenario 8: Empty (invalid) nested .git directory inside parent repo
//
// A directory-form .git without a HEAD file is not a real repository -- git
// itself ignores it. It must be transparent to discovery and attribution:
// checkpoints for files beneath it belong to the enclosing parent repo.
// ===========================================================================

/// A file beneath an empty nested `.git` directory is checkpointed and
/// attributed in the parent repo, exactly as if the empty `.git` dir did not
/// exist.
#[test]
fn test_checkpoint_beneath_empty_nested_git_dir_mock_ai() {
    let workspace = create_unique_workspace("git-ai-empty-nested-git-test");

    let parent_path = workspace.join("parent-repo");
    let parent = TestRepo::new_at_path(&parent_path);
    let mut parent_readme = parent.filename("README.md");
    parent_readme.set_contents(crate::lines!["# Parent Repo"]);
    parent.stage_all_and_commit("initial parent").unwrap();

    // An empty .git directory (e.g. left behind by tooling) is not a repo.
    fs::create_dir_all(parent_path.join("tooling/.git")).unwrap();
    fs::write(
        parent_path.join("tooling/tool.txt"),
        "AI line 1\nAI line 2\n",
    )
    .unwrap();

    let tool_file_abs = parent.canonical_path().join("tooling").join("tool.txt");
    parent
        .git_ai_from_working_dir(
            &parent.canonical_path(),
            &["checkpoint", "mock_ai", tool_file_abs.to_str().unwrap()],
        )
        .expect("checkpoint for file beneath empty nested .git dir should succeed");

    let working_log = parent.current_working_logs();
    let ai_files = working_log.all_ai_touched_files().unwrap_or_default();
    assert!(
        ai_files.iter().any(|f| f.ends_with("tool.txt")),
        "Scenario 8: file beneath an empty nested .git dir should be checkpointed \
         in the parent repo, got: {ai_files:?}"
    );

    parent.stage_all_and_commit("add tool").unwrap();
    let mut tool_file = parent.filename("tooling/tool.txt");
    tool_file.assert_committed_lines(crate::lines!["AI line 1".ai(), "AI line 2".ai()]);

    let _ = fs::remove_dir_all(&workspace);
}

/// Same layout, but the checkpoint runs with CWD *inside* the directory
/// holding the empty `.git`: repository discovery must walk up past the
/// invalid marker and root at the parent repo.
#[test]
fn test_checkpoint_from_cwd_inside_empty_nested_git_dir_mock_ai() {
    let workspace = create_unique_workspace("git-ai-empty-nested-git-cwd-test");

    let parent_path = workspace.join("parent-repo");
    let parent = TestRepo::new_at_path(&parent_path);
    let mut parent_readme = parent.filename("README.md");
    parent_readme.set_contents(crate::lines!["# Parent Repo"]);
    parent.stage_all_and_commit("initial parent").unwrap();

    fs::create_dir_all(parent_path.join("tooling/.git")).unwrap();
    fs::write(
        parent_path.join("tooling/tool.txt"),
        "AI line 1\nAI line 2\n",
    )
    .unwrap();

    let tooling_cwd = parent.canonical_path().join("tooling");
    let tool_file_abs = tooling_cwd.join("tool.txt");
    parent
        .git_ai_from_working_dir(
            &tooling_cwd,
            &["checkpoint", "mock_ai", tool_file_abs.to_str().unwrap()],
        )
        .expect("checkpoint from CWD inside empty nested .git dir should succeed");

    let working_log = parent.current_working_logs();
    let ai_files = working_log.all_ai_touched_files().unwrap_or_default();
    assert!(
        ai_files.iter().any(|f| f.ends_with("tool.txt")),
        "Scenario 8 (cwd inside): file should be checkpointed in the parent repo \
         when CWD is beneath the empty nested .git dir, got: {ai_files:?}"
    );

    parent.stage_all_and_commit("add tool").unwrap();
    let mut tool_file = parent.filename("tooling/tool.txt");
    tool_file.assert_committed_lines(crate::lines!["AI line 1".ai(), "AI line 2".ai()]);

    let _ = fs::remove_dir_all(&workspace);
}
