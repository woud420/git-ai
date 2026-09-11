use super::{ExpectedLineExt, TestRepo, create_unique_workspace, fixture_path, fs, json};

// ===========================================================================
// Scenario 7: Nested subrepo inside parent repo (mock_ai)
//
// Unlike scenarios 1-6 where repos are siblings or in unrelated directories,
// here the subrepo is a subdirectory of the parent repo. Both have their own
// .git/ directory. This tests that path_is_in_workdir correctly detects the
// nested .git boundary and routes checkpoints to the subrepo.
// ===========================================================================

/// When CWD is a parent git repo and the edited file is inside a nested
/// subrepo (subdirectory with its own .git/), the checkpoint must be written
/// to the subrepo, not the parent.
#[test]
fn test_nested_subrepo_single_file_mock_ai() {
    let workspace = create_unique_workspace("git-ai-nested-subrepo-test");

    // Create parent repo
    let parent_path = workspace.join("parent-repo");
    let parent = TestRepo::new_at_path(&parent_path);
    let mut parent_readme = parent.filename("README.md");
    parent_readme.set_contents(crate::lines!["# Parent Repo"]);
    parent.stage_all_and_commit("initial parent").unwrap();

    // Create nested subrepo inside parent repo
    let subrepo_path = parent_path.join("subrepo");
    let subrepo = TestRepo::new_at_path(&subrepo_path);
    let mut subrepo_readme = subrepo.filename("README.md");
    subrepo_readme.set_contents(crate::lines!["# Subrepo"]);
    subrepo.stage_all_and_commit("initial subrepo").unwrap();

    // Write AI content in the subrepo
    fs::write(subrepo_path.join("feature.txt"), "AI line 1\nAI line 2\n").unwrap();

    let subrepo_file = subrepo.canonical_path().join("feature.txt");

    // Checkpoint from parent repo's CWD, targeting file in nested subrepo
    parent
        .git_ai_from_working_dir(
            &parent.canonical_path(),
            &["checkpoint", "mock_ai", subrepo_file.to_str().unwrap()],
        )
        .expect("checkpoint from parent repo CWD for nested subrepo file should succeed");

    // Verify working log was written in the subrepo, not the parent
    let working_log = subrepo.current_working_logs();
    let ai_files = working_log.all_ai_touched_files().unwrap_or_default();
    assert!(
        !ai_files.is_empty(),
        "Scenario 7: Working log entries should exist in the nested subrepo \
         when checkpoint is run from the parent repo's CWD."
    );

    // Commit in the subrepo and verify AI attribution
    let commit = subrepo.stage_all_and_commit("add AI feature").unwrap();
    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "Scenario 7: AI attestations should be present in the nested subrepo \
         when checkpoint was run from the parent repo's CWD."
    );

    let _ = fs::remove_dir_all(&workspace);
}

/// Mixed edits: some files in parent repo, some in nested subrepo.
/// Both repos should get their respective checkpoints.
#[test]
fn test_nested_subrepo_mixed_edits_mock_ai() {
    let workspace = create_unique_workspace("git-ai-nested-mixed-test");

    let parent_path = workspace.join("parent-repo");
    let parent = TestRepo::new_at_path(&parent_path);
    let mut parent_readme = parent.filename("README.md");
    parent_readme.set_contents(crate::lines!["# Parent"]);
    parent.stage_all_and_commit("initial parent").unwrap();

    let subrepo_path = parent_path.join("subrepo");
    let subrepo = TestRepo::new_at_path(&subrepo_path);
    let mut subrepo_readme = subrepo.filename("README.md");
    subrepo_readme.set_contents(crate::lines!["# Subrepo"]);
    subrepo.stage_all_and_commit("initial subrepo").unwrap();

    // Write AI content in both repos
    fs::write(parent_path.join("parent_feature.txt"), "Parent AI\n").unwrap();
    fs::write(subrepo_path.join("subrepo_feature.txt"), "Subrepo AI\n").unwrap();

    let parent_file = parent.canonical_path().join("parent_feature.txt");
    let subrepo_file = subrepo.canonical_path().join("subrepo_feature.txt");

    // Checkpoint from parent CWD with files in both repos
    parent
        .git_ai_from_working_dir(
            &parent.canonical_path(),
            &[
                "checkpoint",
                "mock_ai",
                parent_file.to_str().unwrap(),
                subrepo_file.to_str().unwrap(),
            ],
        )
        .expect("mixed checkpoint (parent + nested subrepo) should succeed");

    // The mixed checkpoint fans out to both repo families; drain both before
    // committing so this test does not race delegated checkpoint processing.
    parent.sync_daemon();
    subrepo.sync_daemon();

    // Verify parent repo has attestation
    let parent_commit = parent.stage_all_and_commit("AI edits in parent").unwrap();
    assert!(
        !parent_commit.authorship_log.attestations.is_empty(),
        "Scenario 7 (mixed): Parent repo should have AI attestations for its own files."
    );

    // Verify subrepo has attestation
    let subrepo_commit = subrepo.stage_all_and_commit("AI edits in subrepo").unwrap();
    assert!(
        !subrepo_commit.authorship_log.attestations.is_empty(),
        "Scenario 7 (mixed): Nested subrepo should have AI attestations for its own files."
    );

    let _ = fs::remove_dir_all(&workspace);
}

/// Multiple nested subrepos inside the same parent repo.
#[test]
fn test_nested_multiple_subrepos_mock_ai() {
    let workspace = create_unique_workspace("git-ai-nested-multi-sub-test");

    let parent_path = workspace.join("parent-repo");
    let parent = TestRepo::new_at_path(&parent_path);
    let mut parent_readme = parent.filename("README.md");
    parent_readme.set_contents(crate::lines!["# Parent"]);
    parent.stage_all_and_commit("initial parent").unwrap();

    let sub1_path = parent_path.join("sub1");
    let sub1 = TestRepo::new_at_path(&sub1_path);
    let mut sub1_readme = sub1.filename("README.md");
    sub1_readme.set_contents(crate::lines!["# Sub1"]);
    sub1.stage_all_and_commit("initial sub1").unwrap();

    let sub2_path = parent_path.join("sub2");
    let sub2 = TestRepo::new_at_path(&sub2_path);
    let mut sub2_readme = sub2.filename("README.md");
    sub2_readme.set_contents(crate::lines!["# Sub2"]);
    sub2.stage_all_and_commit("initial sub2").unwrap();

    fs::write(sub1_path.join("s1.txt"), "AI sub1\n").unwrap();
    fs::write(sub2_path.join("s2.txt"), "AI sub2\n").unwrap();

    let file_s1 = sub1.canonical_path().join("s1.txt");
    let file_s2 = sub2.canonical_path().join("s2.txt");

    parent
        .git_ai_from_working_dir(
            &parent.canonical_path(),
            &[
                "checkpoint",
                "mock_ai",
                file_s1.to_str().unwrap(),
                file_s2.to_str().unwrap(),
            ],
        )
        .expect("checkpoint across multiple nested subrepos should succeed");

    // The checkpoint spans two nested repo families; drain both before either
    // commit so each repo sees its delegated checkpoint.
    sub1.sync_daemon();
    sub2.sync_daemon();

    let commit_s1 = sub1.stage_all_and_commit("AI in sub1").unwrap();
    assert!(
        !commit_s1.authorship_log.attestations.is_empty(),
        "Scenario 7 (multi-sub): sub1 should have AI attestations."
    );

    let commit_s2 = sub2.stage_all_and_commit("AI in sub2").unwrap();
    assert!(
        !commit_s2.authorship_log.attestations.is_empty(),
        "Scenario 7 (multi-sub): sub2 should have AI attestations."
    );

    let _ = fs::remove_dir_all(&workspace);
}

/// Blame verification for nested subrepo: human lines stay human,
/// AI-appended lines show as AI.
#[test]
fn test_nested_subrepo_blame_attribution_mock_ai() {
    let workspace = create_unique_workspace("git-ai-nested-blame-test");

    let parent_path = workspace.join("parent-repo");
    let parent = TestRepo::new_at_path(&parent_path);
    let mut parent_readme = parent.filename("README.md");
    parent_readme.set_contents(crate::lines!["# Parent"]);
    parent.stage_all_and_commit("initial parent").unwrap();

    let subrepo_path = parent_path.join("subrepo");
    let subrepo = TestRepo::new_at_path(&subrepo_path);

    // Initial human content in subrepo
    let mut existing = subrepo.filename("code.txt");
    existing.set_contents(crate::lines!["Human line 1", "Human line 2"]);
    subrepo.stage_all_and_commit("initial subrepo").unwrap();

    // Append AI lines
    fs::write(
        subrepo_path.join("code.txt"),
        "Human line 1\nHuman line 2\nAI appended 1\nAI appended 2\n",
    )
    .unwrap();

    let target_file = subrepo.canonical_path().join("code.txt");

    parent
        .git_ai_from_working_dir(
            &parent.canonical_path(),
            &["checkpoint", "mock_ai", target_file.to_str().unwrap()],
        )
        .expect("nested subrepo blame checkpoint should succeed");

    subrepo
        .stage_all_and_commit("add AI lines from parent CWD")
        .unwrap();

    let mut file = subrepo.filename("code.txt");
    file.assert_lines_and_blame(vec![
        "Human line 1".human(),
        "Human line 2".ai(),
        "AI appended 1".ai(),
        "AI appended 2".ai(),
    ]);

    let _ = fs::remove_dir_all(&workspace);
}

// ===========================================================================
// Scenario 8: Nested subrepo with Claude preset
//
// Mirrors scenario 6 (issue #871) but with the subrepo nested inside the
// parent repo instead of being a sibling repo.
// ===========================================================================

/// Claude Code running in parent repo, editing a file in nested subrepo.
/// Checkpoint should record data in subrepo so commit produces non-empty prompts.
#[test]
fn test_claude_preset_nested_subrepo_records_prompts() {
    let workspace = create_unique_workspace("git-ai-claude-nested-test");

    let parent_path = workspace.join("parent-repo");
    let parent = TestRepo::new_at_path(&parent_path);
    fs::write(parent_path.join("README.md"), "# Parent Repo\n").unwrap();
    parent.stage_all_and_commit("initial parent").unwrap();

    let subrepo_path = parent_path.join("subrepo");
    let mut subrepo = TestRepo::new_at_path(&subrepo_path);

    // Enable prompt sharing for the subrepo
    subrepo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let parent_root = parent.canonical_path();
    let subrepo_root = subrepo.canonical_path();

    let src_dir = subrepo_root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    let target_file = src_dir.join("main.ts");
    fs::write(&target_file, "console.log('hello');\n").unwrap();
    subrepo.stage_all_and_commit("initial subrepo").unwrap();

    // Create transcript file
    let transcript_path = subrepo_root.join("claude-session.jsonl");
    let fixture = fixture_path("example-claude-code.jsonl");
    fs::copy(&fixture, &transcript_path).unwrap();

    // Build Claude PostToolUse hook input with CWD = parent repo
    let hook_input = json!({
        "cwd": parent_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "transcript_path": transcript_path.to_string_lossy().to_string(),
        "tool_input": {
            "file_path": target_file.to_string_lossy().to_string()
        }
    })
    .to_string();

    // Simulate AI edit in subrepo
    fs::write(
        &target_file,
        "console.log('hello');\nconsole.log('AI was here');\n",
    )
    .unwrap();

    // Run checkpoint from parent repo's CWD with Claude preset
    subrepo
        .git_ai_from_working_dir(
            &parent_root,
            &["checkpoint", "claude", "--hook-input", &hook_input],
        )
        .expect("Claude checkpoint from parent CWD for nested subrepo should succeed");

    // Verify working log in subrepo
    let working_log = subrepo.current_working_logs();
    let ai_files = working_log.all_ai_touched_files().unwrap_or_default();
    assert!(
        !ai_files.is_empty(),
        "Scenario 8: Working log entries should exist in the nested subrepo \
         when Claude checkpoint is run from the parent repo's CWD."
    );

    // Commit in subrepo
    let commit = subrepo.stage_all_and_commit("add AI changes").unwrap();

    // Sessions must NOT be empty
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Scenario 8: Sessions should not be empty in nested subrepo's git note \
         when Claude Code is started in parent repo but edits files in subrepo."
    );

    // Attestations must be present
    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "Scenario 8: AI attestations should be present in the nested subrepo."
    );

    let _ = fs::remove_dir_all(&workspace);
}

/// Claude PreToolUse + PostToolUse cycle for nested subrepo.
#[test]
fn test_claude_preset_nested_subrepo_pre_post_cycle() {
    let workspace = create_unique_workspace("git-ai-claude-nested-cycle-test");

    let parent_path = workspace.join("parent-repo");
    let parent = TestRepo::new_at_path(&parent_path);
    fs::write(parent_path.join("README.md"), "# Parent\n").unwrap();
    parent.stage_all_and_commit("initial parent").unwrap();

    let subrepo_path = parent_path.join("subrepo");
    let subrepo = TestRepo::new_at_path(&subrepo_path);

    let parent_root = parent.canonical_path();
    let subrepo_root = subrepo.canonical_path();

    let target_file = subrepo_root.join("feature.txt");
    fs::write(&target_file, "line 1\nline 2\n").unwrap();
    subrepo.stage_all_and_commit("initial subrepo").unwrap();

    let transcript_path = subrepo_root.join("claude-session.jsonl");
    let fixture = fixture_path("example-claude-code.jsonl");
    fs::copy(&fixture, &transcript_path).unwrap();

    // PreToolUse (human checkpoint before AI edit)
    let pre_hook_input = json!({
        "cwd": parent_root.to_string_lossy().to_string(),
        "hook_event_name": "PreToolUse",
        "transcript_path": transcript_path.to_string_lossy().to_string(),
        "tool_input": {
            "file_path": target_file.to_string_lossy().to_string()
        }
    })
    .to_string();

    subrepo
        .git_ai_from_working_dir(
            &parent_root,
            &["checkpoint", "claude", "--hook-input", &pre_hook_input],
        )
        .expect("PreToolUse checkpoint for nested subrepo should succeed");

    // Simulate AI edit
    fs::write(&target_file, "line 1\nline 2\nAI line 3\n").unwrap();

    // PostToolUse (AI checkpoint after edit)
    let post_hook_input = json!({
        "cwd": parent_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "transcript_path": transcript_path.to_string_lossy().to_string(),
        "tool_input": {
            "file_path": target_file.to_string_lossy().to_string()
        }
    })
    .to_string();

    subrepo
        .git_ai_from_working_dir(
            &parent_root,
            &["checkpoint", "claude", "--hook-input", &post_hook_input],
        )
        .expect("PostToolUse checkpoint for nested subrepo should succeed");

    // Commit and verify
    let commit = subrepo.stage_all_and_commit("add AI changes").unwrap();
    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "Scenario 8 (cycle): AI attestations should be present in nested subrepo \
         after PreToolUse + PostToolUse cycle from parent repo's CWD."
    );

    let _ = fs::remove_dir_all(&workspace);
}
