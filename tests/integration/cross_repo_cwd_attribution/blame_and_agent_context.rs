use super::{ExpectedLineExt, TestRepo, create_unique_workspace, fixture_path, fs, json};

// ---------------------------------------------------------------------------
// Additional edge case: blame verification for cross-repo CWD attribution
// ---------------------------------------------------------------------------

/// Verify that blame output correctly shows AI authorship for lines written
/// via cross-repo checkpoint where CWD is an unrelated directory.
#[test]
fn test_cross_repo_cwd_blame_shows_correct_attribution() {
    let repo_cwd = TestRepo::new();
    let repo_target = TestRepo::new();

    // Set up target repo with an existing file
    let mut existing = repo_target.filename("existing.txt");
    existing.set_contents(crate::lines!["Human line 1", "Human line 2"]);
    repo_target
        .stage_all_and_commit("initial commit with human lines")
        .unwrap();

    // Append AI lines to the existing file
    fs::write(
        repo_target.path().join("existing.txt"),
        "Human line 1\nHuman line 2\nAI appended line 1\nAI appended line 2\n",
    )
    .unwrap();

    let target_file_abs = repo_target.canonical_path().join("existing.txt");

    // Checkpoint from the unrelated CWD
    repo_target
        .git_ai_from_working_dir(
            &repo_cwd.canonical_path(),
            &["checkpoint", "mock_ai", target_file_abs.to_str().unwrap()],
        )
        .expect("cross-repo CWD checkpoint should succeed");

    // Commit and verify blame
    repo_target
        .stage_all_and_commit("add AI lines from cross-repo CWD")
        .unwrap();

    let mut file = repo_target.filename("existing.txt");
    file.assert_lines_and_blame(vec![
        "Human line 1".human(),
        "Human line 2".ai(),
        "AI appended line 1".ai(),
        "AI appended line 2".ai(),
    ]);
}

/// Verify blame across multiple repos when CWD is a parent directory.
#[test]
fn test_parent_cwd_blame_correct_across_repos() {
    let workspace = create_unique_workspace("git-ai-parent-blame-test");

    let repo_a_path = workspace.join("svc-a");
    let repo_b_path = workspace.join("svc-b");

    let repo_a = TestRepo::new_at_path(&repo_a_path);
    let repo_b = TestRepo::new_at_path(&repo_b_path);

    // Initial commits with human content
    let mut file_a = repo_a.filename("code.txt");
    file_a.set_contents(crate::lines!["Human A1", "Human A2"]);
    repo_a.stage_all_and_commit("initial A").unwrap();

    let mut file_b = repo_b.filename("code.txt");
    file_b.set_contents(crate::lines!["Human B1"]);
    repo_b.stage_all_and_commit("initial B").unwrap();

    // Write mixed content (human + AI appended)
    fs::write(repo_a_path.join("code.txt"), "Human A1\nHuman A2\nAI A3\n").unwrap();
    fs::write(repo_b_path.join("code.txt"), "Human B1\nAI B2\nAI B3\n").unwrap();

    let abs_a = repo_a.canonical_path().join("code.txt");
    let abs_b = repo_b.canonical_path().join("code.txt");

    // Checkpoint from parent workspace
    repo_a
        .git_ai_from_working_dir(
            &workspace,
            &[
                "checkpoint",
                "mock_ai",
                abs_a.to_str().unwrap(),
                abs_b.to_str().unwrap(),
            ],
        )
        .expect("parent-CWD checkpoint for blame test should succeed");

    // Commit both repos
    repo_a.stage_all_and_commit("AI additions A").unwrap();
    repo_b.stage_all_and_commit("AI additions B").unwrap();

    // Verify blame in repo_a
    let mut blamed_a = repo_a.filename("code.txt");
    blamed_a.assert_lines_and_blame(vec!["Human A1".human(), "Human A2".ai(), "AI A3".ai()]);

    // Verify blame in repo_b
    let mut blamed_b = repo_b.filename("code.txt");
    blamed_b.assert_lines_and_blame(vec!["Human B1".ai(), "AI B2".ai(), "AI B3".ai()]);

    let _ = fs::remove_dir_all(&workspace);
}

// ---------------------------------------------------------------------------
// Scenario 6: Agent preset (Claude) with CWD in repo A, editing files in repo B
// Regression test for issue #871
// ---------------------------------------------------------------------------

/// When Claude Code is started in repo A but edits a file in repo B,
/// the checkpoint should record data in repo B so that committing in repo B
/// produces non-empty prompts in the git note.
#[test]
fn test_claude_preset_cross_repo_cwd_records_prompts_in_target_repo() {
    // repo_cwd is where the agent session runs (repo A)
    let repo_cwd = TestRepo::new();
    // repo_target is where the file is actually edited (repo B)
    let mut repo_target = TestRepo::new();

    // Enable prompt sharing for the target repo
    repo_target.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let cwd_root = repo_cwd.canonical_path();
    let target_root = repo_target.canonical_path();

    // Set up initial commits in both repos
    fs::write(cwd_root.join("README.md"), "# Repo A\n").unwrap();
    repo_cwd.stage_all_and_commit("initial commit A").unwrap();

    let src_dir = target_root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    let target_file = src_dir.join("main.ts");
    fs::write(&target_file, "console.log('hello');\n").unwrap();
    repo_target
        .stage_all_and_commit("initial commit B")
        .unwrap();

    // Create a transcript file that the Claude preset can parse
    let transcript_path = target_root.join("claude-session.jsonl");
    let fixture = fixture_path("example-claude-code.jsonl");
    fs::copy(&fixture, &transcript_path).unwrap();

    // Build hook input JSON simulating Claude Code running from repo A
    // but editing a file in repo B (absolute path)
    let hook_input = json!({
        "cwd": cwd_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "transcript_path": transcript_path.to_string_lossy().to_string(),
        "tool_input": {
            "file_path": target_file.to_string_lossy().to_string()
        }
    })
    .to_string();

    // Simulate AI edit in repo B
    fs::write(
        &target_file,
        "console.log('hello');\nconsole.log('AI was here');\n",
    )
    .unwrap();

    // Run checkpoint from repo A's CWD, with Claude preset hook input
    // pointing to a file in repo B
    repo_target
        .git_ai_from_working_dir(
            &cwd_root,
            &["checkpoint", "claude", "--hook-input", &hook_input],
        )
        .expect("checkpoint from cross-repo CWD should succeed");

    // Verify the working log was written in the target repo
    let working_log = repo_target.current_working_logs();
    let ai_files = working_log.all_ai_touched_files().unwrap_or_default();
    assert!(
        !ai_files.is_empty(),
        "Issue #871 regression: Working log entries should exist in repo B \
         when Claude checkpoint is run from repo A's CWD."
    );

    // Commit in repo B
    let commit = repo_target.stage_all_and_commit("add AI changes").unwrap();

    // The core assertion: sessions must NOT be empty
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Issue #871 regression: Sessions should not be empty in repo B's git note \
         when Claude Code is started in repo A but edits files in repo B."
    );

    // Verify attestations are present too
    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "Issue #871 regression: AI attestations should be present in repo B \
         when checkpoint is run from repo A's CWD with Claude preset."
    );
}

/// Same as above but tests the PreToolUse (human checkpoint) path.
/// The human checkpoint should also correctly record in repo B when CWD is repo A.
#[test]
fn test_claude_preset_cross_repo_cwd_pre_tool_use_records_in_target_repo() {
    let repo_cwd = TestRepo::new();
    let repo_target = TestRepo::new();

    let cwd_root = repo_cwd.canonical_path();
    let target_root = repo_target.canonical_path();

    // Set up initial commits
    fs::write(cwd_root.join("README.md"), "# Repo A\n").unwrap();
    repo_cwd.stage_all_and_commit("initial commit A").unwrap();

    let target_file = target_root.join("feature.txt");
    fs::write(&target_file, "line 1\nline 2\n").unwrap();
    repo_target
        .stage_all_and_commit("initial commit B")
        .unwrap();

    // Create a transcript file
    let transcript_path = target_root.join("claude-session.jsonl");
    let fixture = fixture_path("example-claude-code.jsonl");
    fs::copy(&fixture, &transcript_path).unwrap();

    // PreToolUse hook input (human checkpoint before AI edit)
    let pre_hook_input = json!({
        "cwd": cwd_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "transcript_path": transcript_path.to_string_lossy().to_string(),
        "tool_input": {
            "file_path": target_file.to_string_lossy().to_string()
        }
    })
    .to_string();

    // Run PreToolUse checkpoint from repo A's CWD
    repo_target
        .git_ai_from_working_dir(
            &cwd_root,
            &["checkpoint", "claude", "--hook-input", &pre_hook_input],
        )
        .expect("PreToolUse checkpoint from cross-repo CWD should succeed");

    // Simulate AI edit in repo B
    fs::write(&target_file, "line 1\nline 2\nAI line 3\n").unwrap();

    // PostToolUse hook input
    let post_hook_input = json!({
        "cwd": cwd_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "transcript_path": transcript_path.to_string_lossy().to_string(),
        "tool_input": {
            "file_path": target_file.to_string_lossy().to_string()
        }
    })
    .to_string();

    // Run PostToolUse checkpoint from repo A's CWD
    repo_target
        .git_ai_from_working_dir(
            &cwd_root,
            &["checkpoint", "claude", "--hook-input", &post_hook_input],
        )
        .expect("PostToolUse checkpoint from cross-repo CWD should succeed");

    // Commit in repo B
    let commit = repo_target.stage_all_and_commit("add AI changes").unwrap();

    // Verify attestations are present
    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "Issue #871 regression: AI attestations should be present in repo B \
         when PreToolUse + PostToolUse checkpoints run from repo A's CWD."
    );
}
