use super::{ExpectedLineExt, TestRepo};

#[test]
fn test_diff_invalid_range_format() {
    let repo = TestRepo::new();

    // Create commit
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Content".human()]);
    repo.stage_all_and_commit("Test").unwrap();

    // Try invalid range formats
    let result1 = repo.git_ai(&["diff", "..."]);
    assert!(
        result1.is_err(),
        "diff with '...' should fail (triple dots not supported)"
    );
}

#[test]
fn test_diff_range_start_equals_end() {
    let repo = TestRepo::new();

    // Create commit
    let mut file = repo.filename("same.txt");
    file.set_contents(crate::lines!["Content".human()]);
    let commit = repo.stage_all_and_commit("Test").unwrap();

    // Try range where start equals end
    let range = format!("{}..{}", commit.commit_sha, commit.commit_sha);
    let output = repo
        .git_ai(&["diff", &range])
        .expect("diff with same start/end should succeed");

    // Should show empty diff (no changes between identical commits)
    assert!(
        output.is_empty() || !output.contains("@@"),
        "Diff between same commits should be empty"
    );
}

// ============================================================================
// Edge Cases for File Handling
// ============================================================================

#[test]
fn test_diff_new_file_from_empty() {
    let repo = TestRepo::new();

    // Create initial empty commit using git directly to avoid checkpoint system
    repo.git(&["commit", "--allow-empty", "-m", "Empty initial"])
        .expect("empty commit should succeed");

    // Add new file
    let mut file = repo.filename("new.rs");
    file.set_contents(crate::lines!["fn new() {}".ai()]);
    let commit = repo.stage_all_and_commit("Add new file").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff with new file should succeed");

    // Should show additions
    assert!(output.contains("+"), "Should show additions for new file");
}

#[test]
fn test_diff_deleted_file() {
    let repo = TestRepo::new();

    // Create file
    let mut file = repo.filename("deleted.rs");
    file.set_contents(crate::lines!["fn old() {}".human()]);
    repo.stage_all_and_commit("Add file").unwrap();

    // Delete file
    std::fs::remove_file(repo.path().join("deleted.rs")).unwrap();
    let commit = repo.stage_all_and_commit("Delete file").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff with deleted file should succeed");

    // Should show deletions
    assert!(
        output.contains("-"),
        "Should show deletions for deleted file"
    );
}

#[test]
fn test_diff_renamed_file() {
    let repo = TestRepo::new();

    // Create file
    let mut file = repo.filename("old_name.rs");
    file.set_contents(crate::lines!["fn test() {}".human()]);
    repo.stage_all_and_commit("Add file").unwrap();

    // Rename file via git
    repo.git(&["mv", "old_name.rs", "new_name.rs"]).unwrap();
    let commit = repo.stage_all_and_commit("Rename file").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff with renamed file should succeed");

    // Git should detect rename, diff should handle it
    assert!(!output.is_empty(), "Diff should show file changes");
}

#[test]
fn test_diff_empty_file() {
    let repo = TestRepo::new();

    // Create empty file
    let file_path = repo.path().join("empty.txt");
    std::fs::write(&file_path, "").unwrap();
    repo.stage_all_and_commit("Add empty file").unwrap();

    // Add content to file
    std::fs::write(&file_path, "content\n").unwrap();
    let commit = repo.stage_all_and_commit("Add content").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff with empty file should succeed");

    // Should show addition
    assert!(output.contains("+"), "Should show addition to empty file");
}

// ============================================================================
// Performance and Scalability Tests
// ============================================================================

#[test]
fn test_diff_large_file() {
    let repo = TestRepo::new();

    // Create large file
    let mut file = repo.filename("large.txt");
    let large_content: Vec<_> = (0..1000).map(|i| format!("Line {}", i).human()).collect();
    file.set_contents(large_content.clone());
    repo.stage_all_and_commit("Large file").unwrap();

    // Modify one line in the middle
    let mut modified = large_content;
    modified[500] = "Modified line 500".ai();
    file.set_contents(modified);
    let commit = repo.stage_all_and_commit("Modify large file").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff with large file should succeed");

    // Should handle large file
    assert!(
        output.contains("Modified line 500"),
        "Should show the modified line"
    );
}

#[test]
fn test_diff_many_files() {
    let repo = TestRepo::new();

    // Create many files
    for i in 0..50 {
        let mut file = repo.filename(&format!("file{}.txt", i));
        file.set_contents(crate::lines![format!("Content {}", i).human()]);
    }
    repo.stage_all_and_commit("Many files").unwrap();

    // Modify some files
    for i in 0..10 {
        let mut file = repo.filename(&format!("file{}.txt", i));
        file.set_contents(crate::lines![
            format!("Content {}", i).human(),
            format!("Added {}", i).ai()
        ]);
    }
    let commit = repo.stage_all_and_commit("Modify many").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff with many files should succeed");

    // Should show multiple file diffs
    let diff_count = output.matches("diff --git").count();
    assert!(
        diff_count >= 10,
        "Should have diffs for at least 10 files, got {}",
        diff_count
    );
}

// ============================================================================
// Range Behavior Tests
// ============================================================================

#[test]
fn test_diff_range_multiple_commits() {
    let repo = TestRepo::new();

    // Create series of commits
    let mut file = repo.filename("range.rs");

    file.set_contents(crate::lines!["Line 1".human()]);
    let first = repo.stage_all_and_commit("Commit 1").unwrap();

    file.set_contents(crate::lines!["Line 1".human(), "Line 2".ai()]);
    repo.stage_all_and_commit("Commit 2").unwrap();

    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".human()
    ]);
    repo.stage_all_and_commit("Commit 3").unwrap();

    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".human(),
        "Line 4".ai()
    ]);
    let last = repo.stage_all_and_commit("Commit 4").unwrap();

    // Run diff across all commits
    let range = format!("{}..{}", first.commit_sha, last.commit_sha);
    let output = repo
        .git_ai(&["diff", &range])
        .expect("diff range should succeed");

    // Should show cumulative changes
    assert!(
        output.contains("Line 2") && output.contains("Line 3") && output.contains("Line 4"),
        "Should show all cumulative changes"
    );
}

#[test]
fn test_diff_range_shows_intermediate_changes() {
    let repo = TestRepo::new();

    // Create commits where intermediate changes are made and then reverted
    let mut file = repo.filename("intermediate.rs");

    file.set_contents(crate::lines!["Line 1".human()]);
    let first = repo.stage_all_and_commit("Initial").unwrap();

    file.set_contents(crate::lines!["Line 1".human(), "Temp line".ai()]);
    repo.stage_all_and_commit("Add temp").unwrap();

    file.set_contents(crate::lines!["Line 1".human(), "Final line".ai()]);
    let last = repo.stage_all_and_commit("Replace temp").unwrap();

    // Run diff from first to last
    let range = format!("{}..{}", first.commit_sha, last.commit_sha);
    let output = repo
        .git_ai(&["diff", &range])
        .expect("diff range should succeed");

    // Should show net change (Final line added, not Temp line)
    assert!(
        output.contains("Final line"),
        "Should show final state change"
    );
}

crate::reuse_tests_in_worktree!(
    test_diff_invalid_range_format,
    test_diff_range_start_equals_end,
    test_diff_new_file_from_empty,
    test_diff_deleted_file,
    test_diff_renamed_file,
    test_diff_empty_file,
    test_diff_large_file,
    test_diff_many_files,
    test_diff_range_multiple_commits,
    test_diff_range_shows_intermediate_changes,
);
