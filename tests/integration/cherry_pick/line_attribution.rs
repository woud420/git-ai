use super::{ExpectedLineExt, TestRepo, fs};

/// Test cherry-picking a single AI-authored commit
#[test]
fn test_single_commit_cherry_pick() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Initial content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Get current branch name
    let main_branch = repo.current_branch();

    // Create feature branch with AI-authored changes
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI feature line".ai()]);
    repo.stage_all_and_commit("Add AI feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Switch back to main and cherry-pick the feature commit
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &feature_commit]).unwrap();

    // Verify final file state - hooks should have preserved AI authorship
    file.assert_lines_and_blame(crate::lines![
        "Initial content".ai(),
        "AI feature line".ai(),
    ]);

    // Verify stats
    let stats = repo.stats().unwrap();
    assert_eq!(
        stats.git_diff_added_lines, 2,
        "Should add 1 AI line (+ newline)"
    );
    assert_eq!(stats.ai_additions, 2, "2 AI lines added");
    assert_eq!(stats.ai_accepted, 2, "2 AI lines accepted");
    assert_eq!(stats.human_additions, 0, "0 human lines added");

    // Verify prompt records have correct stats
    let head_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log = git_ai::operations::git::notes_api::read_authorship_v3(
        &git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap()).unwrap(),
        &head_commit,
    )
    .unwrap();

    assert!(
        log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !log.metadata.sessions.is_empty(),
        "Should have at least one session record"
    );
    for (session_id, session_record) in &log.metadata.sessions {
        assert!(
            !session_record.agent_id.tool.is_empty(),
            "Session {} should have a non-empty tool",
            session_id
        );
        assert!(
            !session_record.agent_id.model.is_empty(),
            "Session {} should have a non-empty model",
            session_id
        );
    }
}

/// Test cherry-picking multiple commits in sequence
#[test]
fn test_multiple_commits_cherry_pick() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1", ""]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();

    // Create feature branch with multiple AI-authored commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // First AI commit
    file.insert_at(1, crate::lines!["AI line 2".ai()]);
    repo.stage_all_and_commit("AI commit 1").unwrap();
    let commit1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Second AI commit
    file.insert_at(2, crate::lines!["AI line 3".ai()]);
    repo.stage_all_and_commit("AI commit 2").unwrap();
    let commit2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Third AI commit
    file.insert_at(3, crate::lines!["AI line 4".ai()]);
    repo.stage_all_and_commit("AI commit 3").unwrap();
    let commit3 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Switch back to main and cherry-pick all three commits
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &commit1, &commit2, &commit3])
        .unwrap();

    // Verify final file state - hooks should have preserved AI authorship
    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "AI line 2".ai(),
        "AI line 3".ai(),
        "AI line 4".ai(),
    ]);

    // Verify stats for the last cherry-picked commit
    let stats = repo.stats().unwrap();
    eprintln!("Stats: {:?}", stats);
    // Last commit inserts "AI line 4" - git_diff_added_lines only counts this commit's changes
    // ai_additions is capped by git_diff_added_lines, so it reflects this commit only
    assert_eq!(stats.git_diff_added_lines, 1, "Should have added 1 lines");
    assert_eq!(stats.ai_additions, 1, "At least 1 AI line in this commit");
    assert_eq!(stats.ai_accepted, 1, "1 AI lines accepted in commit");

    // Verify session records exist
    let head_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log = git_ai::operations::git::notes_api::read_authorship_v3(
        &git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap()).unwrap(),
        &head_commit,
    )
    .unwrap();

    assert!(
        log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !log.metadata.sessions.is_empty(),
        "Should have session records"
    );
    for (session_id, session_record) in &log.metadata.sessions {
        assert!(
            !session_record.agent_id.tool.is_empty(),
            "Session {} should have a non-empty tool",
            session_id
        );
        assert!(
            !session_record.agent_id.model.is_empty(),
            "Session {} should have a non-empty model",
            session_id
        );
    }
}

/// Test cherry-picking from branch without AI authorship
#[test]
fn test_cherry_pick_no_ai_authorship() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();
    // Create feature branch with human-only changes (no AI)
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["Human line 2".human()]);
    repo.stage_all_and_commit("Human feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Switch back to main and cherry-pick
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &feature_commit]).unwrap();

    // Verify final file state - should have no AI authorship
    file.assert_lines_and_blame(crate::lines!["Line 1".human(), "Human line 2".human(),]);
}

/// Test that trees-identical fast path works
#[test]
fn test_cherry_pick_identical_trees() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();

    // Create feature branch with AI changes
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI line".ai()]);
    repo.stage_all_and_commit("AI feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Add another commit on feature (just to have a parent)
    file.insert_at(2, crate::lines!["More AI".ai()]);
    repo.stage_all_and_commit("More AI").unwrap();

    // Cherry-pick the first feature commit to main
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", &feature_commit]).unwrap();

    // Verify final file state - hooks should have preserved AI authorship
    file.assert_lines_and_blame(crate::lines!["Line 1".ai(), "AI line".ai(),]);
}

/// Test cherry-pick where some commits become empty (already applied)
#[test]
fn test_cherry_pick_empty_commits() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let main_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["Feature line".ai()]);
    repo.stage_all_and_commit("Add feature").unwrap();
    let feature_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Manually apply the same change to main
    repo.git(&["checkout", &main_branch]).unwrap();

    // Get a fresh TestFile after branch switch - it will auto-populate from the existing file
    let mut file_on_main = repo.filename("file.txt");
    file_on_main.insert_at(1, crate::lines!["Feature line".human()]);
    repo.stage_all_and_commit("Apply feature manually").unwrap();

    // Try to cherry-pick the feature commit (should become empty or conflict)
    let result = repo.git(&["cherry-pick", &feature_commit]);

    // Git might succeed and skip the empty commit, or it might create a conflict
    // The key is that it shouldn't crash
    match result {
        Ok(_) => {
            // Empty commit was skipped successfully
        }
        Err(_) => {
            // Git reported an error (conflict or empty commit)
            // Abort the cherry-pick to clean up
            let _ = repo.git(&["cherry-pick", "--abort"]);
        }
    }

    // Verify final file state - content should be preserved
    let actual_content = repo.read_file("file.txt").unwrap();
    assert_eq!(
        actual_content.trim(),
        "Line 1\nFeature line",
        "File content should be preserved after cherry-pick/abort"
    );
}

#[test]
fn test_cherry_pick_no_commit_defers_to_final_commit_tree() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("file.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    fs::write(&file_path, "base\nAI picked line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file.txt"]).unwrap();
    repo.stage_all_and_commit("ai source").unwrap();
    let source_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git(&["cherry-pick", "--no-commit", &source_commit])
        .unwrap();

    fs::write(&file_path, "base\nAI picked line\nlate untracked line\n").unwrap();
    repo.git(&["add", "file.txt"]).unwrap();
    repo.commit("commit no-commit cherry-pick with later edit")
        .unwrap();

    let mut file = repo.filename("file.txt");
    file.assert_committed_lines(crate::lines![
        "base".unattributed_human(),
        "AI picked line".ai(),
        "late untracked line".unattributed_human(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_single_commit_cherry_pick,
    test_multiple_commits_cherry_pick,
    test_cherry_pick_no_ai_authorship,
    test_cherry_pick_identical_trees,
    test_cherry_pick_empty_commits,
    test_cherry_pick_no_commit_defers_to_final_commit_tree,
);
