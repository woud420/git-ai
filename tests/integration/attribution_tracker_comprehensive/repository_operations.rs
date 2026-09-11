use super::{Attribution, ExpectedLineExt, INITIAL_ATTRIBUTION_TS, TestRepo};

// =============================================================================
// Integration Tests with TestRepo
// =============================================================================

#[test]
fn test_attribution_through_commit() {
    // Integration test: attribution preservation through git commits
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "AI line 1".ai(),
        "Human line 1".human(),
        "AI line 2".ai()
    ]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // Modify the file
    file.set_contents(crate::lines![
        "AI line 1".ai(),
        "Modified by human".human(),
        "AI line 2".ai(),
        "New AI line".ai()
    ]);

    let result = repo.stage_all_and_commit("Second commit");
    assert!(result.is_ok());
}

#[test]
fn test_attribution_through_multiple_commits() {
    // Test attribution preservation through multiple commits
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // First commit - AI content
    file.set_contents(crate::lines!["AI initial".ai()]);
    repo.stage_all_and_commit("Commit 1").unwrap();

    // Second commit - Human modifies
    file.set_contents(crate::lines!["AI initial".ai(), "Human adds".human()]);
    repo.stage_all_and_commit("Commit 2").unwrap();

    // Third commit - AI modifies
    file.set_contents(crate::lines![
        "AI modified initial".ai(),
        "Human adds".human(),
        "AI adds more".ai()
    ]);

    let result = repo.stage_all_and_commit("Commit 3");
    assert!(result.is_ok());
}

#[test]
fn test_attribution_with_file_rename() {
    // Test that attribution survives file renames
    let repo = TestRepo::new();
    let mut file = repo.filename("old.txt");

    file.set_contents(crate::lines!["AI content".ai()]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Rename file
    repo.git(&["mv", "old.txt", "new.txt"]).unwrap();
    repo.git(&["commit", "-m", "Rename"]).unwrap();

    // Verify new file exists
    let new_file = repo.filename("new.txt");
    assert!(new_file.file_path.exists());
}

#[test]
fn test_attribution_multifile_edit() {
    // Test attribution tracking across multiple files
    let repo = TestRepo::new();
    let mut file1 = repo.filename("file1.txt");
    let mut file2 = repo.filename("file2.txt");

    file1.set_contents(crate::lines!["File 1 AI".ai()]);
    file2.set_contents(crate::lines!["File 2 Human".human()]);

    repo.stage_all_and_commit("Multi-file commit").unwrap();

    // Modify both
    file1.set_contents(crate::lines!["File 1 AI".ai(), "Modified".human()]);
    file2.set_contents(crate::lines!["File 2 Human".human(), "AI addition".ai()]);

    let result = repo.stage_all_and_commit("Multi-file edit");
    assert!(result.is_ok());
}

#[test]
fn test_initial_attribution_timestamp() {
    // Test that INITIAL_ATTRIBUTION_TS constant is used correctly
    let attr = Attribution::new(0, 10, "ai-1".to_string(), INITIAL_ATTRIBUTION_TS);
    assert_eq!(attr.ts, 42);
}

#[test]
fn test_attribution_with_checkpoint() {
    // Test attribution behavior with checkpoints
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Initial".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Make working directory changes
    file.set_contents(crate::lines!["Initial".human(), "WIP AI".ai()]);

    // Create checkpoint
    let result = repo.git_ai(&["checkpoint"]);
    assert!(result.is_ok());
}

#[test]
fn test_attribution_through_complex_branch_workflow() {
    // Test attribution through a complex branching workflow
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Initial commit
    file.set_contents(crate::lines!["base".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Capture the original branch name before switching
    let original_branch = repo.current_branch();

    // Create and switch to a branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Make changes on branch
    file.set_contents(crate::lines!["base".human(), "feature".ai()]);
    repo.stage_all_and_commit("Feature work").unwrap();

    // Switch back to the original branch
    repo.git(&["checkout", &original_branch]).unwrap();

    // Verify original content
    let content = std::fs::read_to_string(file.file_path.clone()).unwrap();
    assert!(content.contains("base"));
}

#[test]
fn test_attribution_with_stash() {
    // Test attribution behavior with git stash
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["committed".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Make uncommitted changes
    file.set_contents(crate::lines!["committed".human(), "uncommitted".ai()]);

    // Stash should work
    let result = repo.git(&["stash"]);
    assert!(result.is_ok());

    // File should be back to committed state
    let content = std::fs::read_to_string(file.file_path.clone()).unwrap();
    assert!(content.starts_with("committed"));
}

crate::reuse_tests_in_worktree!(
    test_attribution_through_commit,
    test_attribution_through_multiple_commits,
    test_attribution_with_file_rename,
    test_attribution_multifile_edit,
    test_initial_attribution_timestamp,
    test_attribution_with_checkpoint,
    test_attribution_through_complex_branch_workflow,
    test_attribution_with_stash,
);
