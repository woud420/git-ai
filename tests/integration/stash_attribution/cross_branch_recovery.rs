use super::{ExpectedLineExt, TestRepo};

#[test]
fn test_stash_pop_onto_head_with_ai_changes() {
    // Test that popping stash onto a HEAD with AI changes preserves both attributions
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create file1 with AI content from first session
    let mut file1 = repo.filename("file1.txt");
    file1.set_contents(vec!["file1 line 1".ai(), "file1 line 2".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash file1
    repo.git(&["stash"]).expect("stash should succeed");
    assert!(repo.read_file("file1.txt").is_none());

    // Now create file2 with AI content and commit it to HEAD
    let mut file2 = repo.filename("file2.txt");
    file2.set_contents(vec![
        "file2 line 1".ai(),
        "file2 line 2".ai(),
        "file2 line 3".ai(),
    ]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    let head_commit = repo
        .stage_all_and_commit("add file2 with AI")
        .expect("commit should succeed");

    // Verify HEAD has AI attribution
    file2.assert_lines_and_blame(vec![
        "file2 line 1".ai(),
        "file2 line 2".ai(),
        "file2 line 3".ai(),
    ]);
    assert!(
        !head_commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in HEAD commit"
    );

    // Pop the stash (file1 with AI attribution from stash)
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit the popped changes
    let final_commit = repo
        .stage_all_and_commit("apply stash onto HEAD with AI")
        .expect("commit should succeed");

    // Verify BOTH files maintain their AI attributions:
    // file1 should have AI attribution from the stash
    file1.assert_lines_and_blame(vec!["file1 line 1".ai(), "file1 line 2".ai()]);

    // file2 should STILL have AI attribution (unchanged from HEAD)
    file2.assert_lines_and_blame(vec![
        "file2 line 1".ai(),
        "file2 line 2".ai(),
        "file2 line 3".ai(),
    ]);

    // The authorship log should track file1 (the new changes from stash)
    // file2 should already be in the repo from the previous commit
    assert!(
        final_commit
            .authorship_log
            .attestations
            .iter()
            .any(|a| a.file_path.ends_with("file1.txt")),
        "Expected file1.txt in authorship log"
    );
}

#[test]
fn test_stash_pop_across_branches() {
    // Test that AI attributions are preserved when stashing, switching branches, and popping
    let repo = TestRepo::new();

    // Create initial commit on main branch
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with existing human content
    repo.human_edit("example.txt", "line 1\nline 2\nline 3\n");
    let mut example = repo.filename("example.txt");
    repo.stage_all_and_commit("add example file")
        .expect("commit should succeed");

    // Add 5 AI-generated lines at the bottom
    example.set_contents(vec![
        "line 1".human(),
        "line 2".human(),
        "line 3".human(),
        "AI line 1".ai(),
        "AI line 2".ai(),
        "AI line 3".ai(),
        "AI line 4".ai(),
        "AI line 5".ai(),
    ]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the AI changes
    repo.git(&["stash"]).expect("stash should succeed");

    // Verify file reverted to 3 lines
    let content = repo.read_file("example.txt").expect("file should exist");
    assert_eq!(
        content.lines().count(),
        3,
        "Should have reverted to 3 lines"
    );

    // Create and checkout a new branch
    repo.git(&["checkout", "-b", "feature-branch"])
        .expect("should create and checkout new branch");

    // Pop the stash on the new branch
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit the changes on the new branch
    let commit = repo
        .stage_all_and_commit("apply AI changes on feature branch")
        .expect("commit should succeed");

    // Verify all AI attributions are preserved
    example.assert_lines_and_blame(vec![
        "line 1".human(),
        "line 2".human(),
        "line 3".human(),
        "AI line 1".ai(),
        "AI line 2".ai(),
        "AI line 3".ai(),
        "AI line 4".ai(),
        "AI line 5".ai(),
    ]);

    // Should have AI prompts in authorship log
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_pop_across_branches_with_conflict() {
    // Test that AI attributions are preserved when resolving conflicts after stash pop across branches
    let repo = TestRepo::new();

    // Create initial commit on main branch
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with existing content
    repo.human_edit("example.txt", "line 1\nline 2\nline 3\n");
    let mut example = repo.filename("example.txt");
    repo.stage_all_and_commit("add example file")
        .expect("commit should succeed");

    // Add 5 AI-generated lines at the bottom
    example.set_contents(vec![
        "line 1".human(),
        "line 2".human(),
        "line 3".human(),
        "AI line 1".ai(),
        "AI line 2".ai(),
        "AI line 3".ai(),
        "AI line 4".ai(),
        "AI line 5".ai(),
    ]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the AI changes
    repo.git(&["stash"]).expect("stash should succeed");

    // Create and checkout a new branch
    repo.git(&["checkout", "-b", "feature-branch"])
        .expect("should create and checkout new branch");

    // Make conflicting changes on the new branch (add different content at the bottom)
    example.set_contents(vec![
        "line 1".human(),
        "line 2".human(),
        "line 3".human(),
        "feature line 1".ai(),
        "feature line 2".ai(),
    ]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.stage_all_and_commit("add feature content")
        .expect("commit should succeed");

    // Try to pop the stash - this will create a conflict
    let _result = repo.git(&["stash", "pop"]);

    // Verify there's a conflict
    let content = repo.read_file("example.txt").expect("file should exist");
    assert!(
        content.contains("<<<<<<<") || content.contains(">>>>>>>"),
        "Expected conflict markers in file"
    );

    // Resolve the conflict by keeping both (feature branch lines + stashed AI lines)
    example.set_contents(vec![
        "line 1".human(),
        "line 2".human(),
        "line 3".human(),
        "feature line 1".ai(),
        "feature line 2".ai(),
        "AI line 1".ai(),
        "AI line 2".ai(),
        "AI line 3".ai(),
        "AI line 4".ai(),
        "AI line 5".ai(),
    ]);

    // Mark as resolved and commit
    repo.git(&["add", "example.txt"])
        .expect("should be able to add resolved file");

    let commit = repo
        .stage_all_and_commit("resolved conflict keeping both changes")
        .expect("commit should succeed");

    // Verify all AI attributions are preserved for both sets of changes
    example.assert_lines_and_blame(vec![
        "line 1".human(),
        "line 2".human(),
        "line 3".human(),
        "feature line 1".ai(),
        "feature line 2".ai(),
        "AI line 1".ai(),
        "AI line 2".ai(),
        "AI line 3".ai(),
        "AI line 4".ai(),
        "AI line 5".ai(),
    ]);

    // Should have AI prompts in authorship log
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_branch_preserves_ai_attribution() {
    // ISSUE-009: git stash branch loses AI attribution
    // git stash branch creates a new branch at the stash parent, applies the stash, and drops it.
    // The post_stash_hook must handle the "branch" subcommand to restore attribution.
    //
    // Key: we make a commit AFTER stashing so HEAD advances. git stash branch then
    // resets HEAD to the stash parent, so the working log keyed to the advanced HEAD
    // is irrelevant. Only the stash note (refs/notes/ai-stash) can provide attribution.
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with AI attribution
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["ai line 1".ai(), "ai line 2".ai(), "ai line 3".ai()]);

    // Run checkpoint to track AI attribution
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the changes
    repo.git(&["stash", "push", "-m", "ai-work"])
        .expect("stash should succeed");

    // Verify file is gone
    assert!(repo.read_file("example.txt").is_none());

    // Make a commit to advance HEAD past the stash parent.
    // This ensures that git stash branch will reset HEAD to the stash parent,
    // invalidating any working log entries keyed to the current HEAD.
    let mut other = repo.filename("other.txt");
    other.set_contents(vec!["some other work".human()]);
    repo.stage_all_and_commit("advance HEAD past stash parent")
        .expect("commit should succeed");

    // Use git stash branch to create a new branch from the stash.
    // This resets HEAD to the stash parent commit and applies the stash.
    repo.git(&["stash", "branch", "new-feature", "stash@{0}"])
        .expect("stash branch should succeed");

    // Verify file is back on the new branch
    assert!(
        repo.read_file("example.txt").is_some(),
        "example.txt should exist after stash branch"
    );

    // Commit the changes on the new branch
    let commit = repo
        .stage_all_and_commit("apply stash via branch")
        .expect("commit should succeed");

    // Verify AI attribution is preserved
    example.assert_lines_and_blame(vec!["ai line 1".ai(), "ai line 2".ai(), "ai line 3".ai()]);

    // Check authorship log has AI prompts
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log after stash branch"
    );
}

crate::reuse_tests_in_worktree!(
    test_stash_pop_onto_head_with_ai_changes,
    test_stash_pop_across_branches,
    test_stash_pop_across_branches_with_conflict,
    test_stash_branch_preserves_ai_attribution,
);
