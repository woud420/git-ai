use super::{ExpectedLineExt, TestRepo};

#[test]
fn test_stash_apply_named_reference() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create first stash
    let mut file1 = repo.filename("file1.txt");
    file1.set_contents(vec!["first stash".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.git(&["stash"]).expect("first stash should succeed");

    // Create second stash
    let mut file2 = repo.filename("file2.txt");
    file2.set_contents(vec!["second stash".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.git(&["stash"]).expect("second stash should succeed");

    // Apply the first stash (stash@{1})
    repo.git(&["stash", "apply", "stash@{1}"])
        .expect("stash apply stash@{1} should succeed");

    // Verify file1 is back
    assert!(repo.read_file("file1.txt").is_some());
    assert!(repo.read_file("file2.txt").is_none());

    // Commit and verify attribution
    let commit = repo
        .stage_all_and_commit("apply first stash")
        .expect("commit should succeed");

    file1.assert_lines_and_blame(vec!["first stash".ai()]);

    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_pop_with_existing_stack_entries() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    let mut first = repo.filename("first.txt");
    first.set_contents(vec!["first stash line".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.git(&["stash", "push", "-m", "first"])
        .expect("first stash should succeed");

    let mut second = repo.filename("second.txt");
    second.set_contents(vec!["second stash line".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.git(&["stash", "push", "-m", "second"])
        .expect("second stash should succeed");

    // Pop when stash stack still has another entry (non-empty -> non-empty on some Git versions).
    repo.git(&["stash", "pop"])
        .expect("first pop should succeed");
    let first_pop_commit = repo
        .stage_all_and_commit("apply top stash entry")
        .expect("commit after first pop should succeed");

    second.assert_lines_and_blame(vec!["second stash line".ai()]);
    assert!(
        !first_pop_commit.authorship_log.metadata.sessions.is_empty(),
        "expected sessions for first pop commit"
    );

    // Pop remaining stash entry and verify attribution still restores correctly.
    repo.git(&["stash", "pop"])
        .expect("second pop should succeed");
    let second_pop_commit = repo
        .stage_all_and_commit("apply remaining stash entry")
        .expect("commit after second pop should succeed");

    first.assert_lines_and_blame(vec!["first stash line".ai()]);
    assert!(
        !second_pop_commit
            .authorship_log
            .metadata
            .sessions
            .is_empty(),
        "expected sessions for second pop commit"
    );
}

#[test]
fn test_stash_pop_default_reference() {
    // Test that stash pop defaults to stash@{0}
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create AI content
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["AI content".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash without explicit reference
    repo.git(&["stash"]).expect("stash should succeed");

    // Pop without explicit reference (should use stash@{0})
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit and verify
    let commit = repo
        .stage_all_and_commit("apply default stash")
        .expect("commit should succeed");

    example.assert_lines_and_blame(vec!["AI content".ai()]);

    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_pop_empty_repo() {
    // Test that stash operations don't crash on edge cases
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Try to pop when there's no stash - should fail gracefully
    let result = repo.git(&["stash", "pop"]);
    assert!(result.is_err(), "Should fail when no stash exists");
}

#[test]
fn test_stash_mixed_staged_and_unstaged() {
    // Test stashing with a mix of staged and unstaged changes
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with AI content
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["staged line 1".ai(), "staged line 2".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stage these changes
    repo.git(&["add", "example.txt"])
        .expect("should stage example.txt");

    // Now add more unstaged changes
    example.set_contents(vec![
        "staged line 1".ai(),
        "staged line 2".ai(),
        "unstaged line 3".ai(),
        "unstaged line 4".ai(),
    ]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash both staged and unstaged (git stash by default stashes both)
    repo.git(&["stash", "--include-untracked"])
        .expect("stash should succeed");

    // Verify file is back to original state (doesn't exist)
    assert!(repo.read_file("example.txt").is_none());

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit all changes
    let commit = repo
        .stage_all_and_commit("apply mixed stash")
        .expect("commit should succeed");

    // All lines should have AI attribution preserved (both staged and unstaged)
    example.assert_lines_and_blame(vec![
        "staged line 1".ai(),
        "staged line 2".ai(),
        "unstaged line 3".ai(),
        "unstaged line 4".ai(),
    ]);

    // Should have AI prompts
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

crate::reuse_tests_in_worktree!(
    test_stash_apply_named_reference,
    test_stash_pop_with_existing_stack_entries,
    test_stash_pop_default_reference,
    test_stash_pop_empty_repo,
    test_stash_mixed_staged_and_unstaged,
);
