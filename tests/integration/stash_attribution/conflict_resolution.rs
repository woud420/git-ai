use super::{ExpectedLineExt, TestRepo};

#[test]
fn test_stash_pop_with_conflict() {
    // Test that attribution is preserved when there's a conflict during stash pop
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with mixed human and AI content
    let mut example = repo.filename("example.txt");
    example.set_contents(vec![
        "header".human(),
        "line 1 AI".ai(),
        "line 2 AI".ai(),
        "footer".human(),
    ]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the changes
    repo.git(&["stash"]).expect("stash should succeed");

    // Now create a conflicting version with different mixed content
    example.set_contents(vec![
        "header".human(),
        "line 1 different".ai(),
        "line 2 different".ai(),
        "footer".human(),
    ]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.stage_all_and_commit("conflicting changes")
        .expect("commit should succeed");

    // Try to pop - this WILL create a conflict
    let _result = repo.git(&["stash", "pop"]);

    // Verify there's a conflict
    let content = repo.read_file("example.txt").expect("file should exist");
    assert!(
        content.contains("<<<<<<<"),
        "Expected conflict markers in file, got: {}",
        content
    );

    // Manually resolve the conflict by taking parts from both versions
    example.set_contents(vec![
        "header".human(),        // from both (same)
        "line 1 AI".ai(),        // from stash
        "line 2 different".ai(), // from HEAD
        "footer".human(),        // from both (same)
    ]);

    // Mark as resolved and commit
    repo.git(&["add", "example.txt"])
        .expect("should be able to add resolved file");

    let _commit = repo
        .stage_all_and_commit("resolved conflict")
        .expect("commit should succeed");

    // Verify mixed human and AI attributions are preserved
    example.assert_lines_and_blame(vec![
        "header".human(),
        "line 1 AI".ai(),
        "line 2 different".ai(),
        "footer".human(),
    ]);
}

#[test]
fn test_stash_pop_conflict_preserves_ai_attribution_without_new_checkpoint() {
    // ISSUE-010: git stash pop with conflict loses all AI attribution
    // When git stash pop encounters a conflict, git exits with code 1.
    // The post_stash_hook bails on !exit_status.success(), never restoring attribution.
    // This test resolves conflicts by writing files directly (no checkpoint),
    // so it verifies the stash attribution was properly restored by the hook.
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with human content and commit it
    let mut conflict_file = repo.filename("conflict.txt");
    conflict_file.set_contents(vec!["original line".human()]);
    repo.stage_all_and_commit("add conflict file")
        .expect("commit should succeed");

    // AI edits the file (adds lines)
    conflict_file.set_contents(vec![
        "original line".human(),
        "ai addition 1".ai(),
        "ai addition 2".ai(),
        "ai addition 3".ai(),
    ]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the AI changes
    repo.git(&["stash", "push", "-m", "ai-changes"])
        .expect("stash should succeed");

    // Make a conflicting human commit on the same file
    // Write the file directly to avoid creating AI checkpoints
    std::fs::write(
        repo.path().join("conflict.txt"),
        "original line\nhuman edit on same file\n",
    )
    .expect("write should succeed");
    repo.git(&["add", "-A"]).expect("add should succeed");
    repo.git_ai(&["checkpoint", "--"])
        .expect("human checkpoint should succeed");
    repo.stage_all_and_commit("human conflicting edit")
        .expect("commit should succeed");

    // Try to pop the stash - this will create a conflict (exit code 1)
    let result = repo.git(&["stash", "pop"]);
    // stash pop with conflict returns error
    assert!(result.is_err(), "stash pop should fail due to conflict");

    // Verify there are conflict markers
    let content = repo.read_file("conflict.txt").expect("file should exist");
    assert!(
        content.contains("<<<<<<<") || content.contains(">>>>>>>"),
        "Expected conflict markers in file, got: {}",
        content
    );

    // Resolve the conflict manually by writing the file directly (NO checkpoint, NO set_contents)
    // This simulates a user resolving conflict in their editor without AI assistance
    std::fs::write(
        repo.path().join("conflict.txt"),
        "original line\nhuman edit on same file\nai addition 1\nai addition 2\nai addition 3\n",
    )
    .expect("write should succeed");

    // Mark as resolved and commit
    repo.git(&["add", "conflict.txt"])
        .expect("should be able to add resolved file");

    let commit = repo
        .stage_all_and_commit("resolved conflict")
        .expect("commit should succeed");

    // The AI lines from the stash should still be attributed to AI
    // This will fail if the post_stash_hook bailed on exit code 1
    // and never restored attribution from refs/notes/ai-stash
    conflict_file.assert_lines_and_blame(vec![
        "original line".human(),
        "human edit on same file".human(),
        "ai addition 1".ai(),
        "ai addition 2".ai(),
        "ai addition 3".ai(),
    ]);

    assert!(
        commit
            .authorship_log
            .attestations
            .iter()
            .any(|a| a.file_path.ends_with("conflict.txt")),
        "Expected conflict.txt in authorship log attestations - stash attribution was not restored"
    );

    // Check that AI prompts are present (from the stash attribution)
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log - stash attribution was lost due to conflict exit code"
    );
}

crate::reuse_tests_in_worktree!(
    test_stash_pop_with_conflict,
    test_stash_pop_conflict_preserves_ai_attribution_without_new_checkpoint,
);
