use super::{ExpectedLineExt, TestRepo, assert_diff_lines_exact, parse_diff_output};

#[test]
fn test_diff_commit_range() {
    let repo = TestRepo::new();

    // First commit
    let mut file = repo.filename("range.txt");
    file.set_contents(crate::lines!["Line 1".human()]);
    let first = repo.stage_all_and_commit("First commit").unwrap();

    // Second commit
    file.set_contents(crate::lines!["Line 1".human(), "Line 2".ai()]);
    repo.stage_all_and_commit("Second commit").unwrap();

    // Third commit
    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".human()
    ]);
    let third = repo.stage_all_and_commit("Third commit").unwrap();

    // Run git-ai diff with range
    let range = format!("{}..{}", first.commit_sha, third.commit_sha);
    let output = repo
        .git_ai(&["diff", &range])
        .expect("git-ai diff range should succeed");

    // Verify output
    assert!(output.contains("diff --git"), "Should contain diff header");
    assert!(output.contains("range.txt"), "Should mention the file");
    assert!(
        output.contains("+Line 2") || output.contains("Line 2"),
        "Should show added line"
    );
    assert!(
        output.contains("+Line 3") || output.contains("Line 3"),
        "Should show added line"
    );
}

#[test]
fn test_diff_two_positional_revisions_uses_git_range_semantics() {
    let repo = TestRepo::new();

    // Ensure the "from" commit has a parent so the regression catches accidental from^..from behavior.
    repo.git(&["commit", "--allow-empty", "-m", "Empty initial"])
        .expect("empty commit should succeed");

    let mut file = repo.filename("range_positional.txt");
    file.set_contents(crate::lines!["BASE".human()]);
    let from = repo.stage_all_and_commit("Base commit").unwrap();

    file.set_contents(crate::lines![
        "BASE".human(),
        "AI line 1".ai(),
        "AI line 2".ai()
    ]);
    let to = repo.stage_all_and_commit("Append lines").unwrap();

    let plain_git_diff = repo
        .git_og(&["--no-pager", "diff", &from.commit_sha, &to.commit_sha])
        .expect("plain git diff should succeed");
    assert!(
        plain_git_diff.contains("+AI line 1") && plain_git_diff.contains("+AI line 2"),
        "plain git diff sanity check failed:\n{}",
        plain_git_diff
    );
    assert!(
        !plain_git_diff.contains("new file mode"),
        "plain git diff should not treat this as a new file:\n{}",
        plain_git_diff
    );

    let git_ai_diff = repo
        .git_ai(&["diff", &from.commit_sha, &to.commit_sha])
        .expect("git-ai diff should support two positional revisions");

    assert!(
        git_ai_diff.contains("+AI line 1") && git_ai_diff.contains("+AI line 2"),
        "git-ai diff should include net additions between from/to commits:\n{}",
        git_ai_diff
    );
    assert!(
        !git_ai_diff.contains("new file mode") && !git_ai_diff.contains("--- /dev/null"),
        "git-ai diff should not fallback to from^..from behavior:\n{}",
        git_ai_diff
    );
}

#[test]
fn test_diff_multiple_files() {
    let repo = TestRepo::new();

    // Initial commit
    let mut file1 = repo.filename("file1.txt");
    let mut file2 = repo.filename("file2.txt");
    file1.set_contents(crate::lines!["File 1 line 1".human()]);
    file2.set_contents(crate::lines!["File 2 line 1".human()]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Modify both files
    file1.set_contents(crate::lines!["File 1 line 1".human(), "File 1 line 2".ai()]);
    file2.set_contents(crate::lines![
        "File 2 line 1".human(),
        "File 2 line 2".human()
    ]);
    let commit = repo.stage_all_and_commit("Modify both files").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff should succeed");

    // Should show both files
    assert!(output.contains("file1.txt"), "Should mention file1");
    assert!(output.contains("file2.txt"), "Should mention file2");

    // Should have multiple diff sections
    let diff_count = output.matches("diff --git").count();
    assert_eq!(diff_count, 2, "Should have 2 diff sections");
}

#[test]
fn test_diff_initial_commit() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("initial.txt");
    file.set_contents(crate::lines!["Initial line".ai()]);
    let commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // Run diff on initial commit (should compare to empty tree)
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff on initial commit should succeed");

    // Parse and verify exact sequence
    let lines = parse_diff_output(&output);

    // Should have exactly 1 addition, no deletions
    assert_diff_lines_exact(
        &lines,
        &[
            ("+", "Initial line", Some("ai")), // Only addition with AI attribution
        ],
    );
}

#[test]
fn test_diff_with_head_ref() {
    let repo = TestRepo::new();

    // Initial commit
    let mut file = repo.filename("head_test.txt");
    file.set_contents(crate::lines!["Line 1".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Second commit
    file.set_contents(crate::lines!["Line 1".human(), "Line 2".ai()]);
    repo.stage_all_and_commit("Add line").unwrap();

    // Run diff using HEAD
    let output = repo
        .git_ai(&["diff", "HEAD"])
        .expect("git-ai diff HEAD should succeed");

    // Should work with HEAD reference
    assert!(output.contains("diff --git"), "Should contain diff header");
    assert!(output.contains("head_test.txt"), "Should mention the file");
}

#[test]
fn test_diff_json_include_stats_rejects_commit_ranges() {
    let repo = TestRepo::new();

    let mut file = repo.filename("range_stats.txt");
    file.set_contents(crate::lines!["line 1".human()]);
    let first = repo.stage_all_and_commit("Commit 1").unwrap();

    file.set_contents(crate::lines!["line 1".human(), "line 2".ai()]);
    let second = repo.stage_all_and_commit("Commit 2").unwrap();

    let range = format!("{}..{}", first.commit_sha, second.commit_sha);
    let result = repo.git_ai(&["diff", &range, "--json", "--include-stats"]);
    assert!(
        result.is_err(),
        "--include-stats should be rejected for commit ranges"
    );
}

#[test]
fn test_diff_range_multiple_commits() {
    let repo = TestRepo::new();

    // First commit
    let mut file = repo.filename("multi.txt");
    file.set_contents(crate::lines!["Line 1".human()]);
    let first = repo.stage_all_and_commit("First").unwrap();

    // Second commit
    file.set_contents(crate::lines!["Line 1".human(), "Line 2".ai()]);
    repo.stage_all_and_commit("Second").unwrap();

    // Third commit
    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".human()
    ]);
    repo.stage_all_and_commit("Third").unwrap();

    // Fourth commit
    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".human(),
        "Line 4".ai()
    ]);
    let fourth = repo.stage_all_and_commit("Fourth").unwrap();

    // Run diff across multiple commits
    let range = format!("{}..{}", first.commit_sha, fourth.commit_sha);
    let output = repo
        .git_ai(&["diff", &range])
        .expect("git-ai diff multi-commit range should succeed");

    // Should show cumulative changes
    assert!(output.contains("+Line 2"), "Should show Line 2 addition");
    assert!(output.contains("+Line 3"), "Should show Line 3 addition");
    assert!(output.contains("+Line 4"), "Should show Line 4 addition");

    // Should have attribution markers
    assert!(
        output.contains("🤖") || output.contains("👤"),
        "Should have attribution markers"
    );
}

crate::reuse_tests_in_worktree!(
    test_diff_commit_range,
    test_diff_multiple_files,
    test_diff_initial_commit,
    test_diff_with_head_ref,
    test_diff_json_include_stats_rejects_commit_ranges,
    test_diff_range_multiple_commits,
);
