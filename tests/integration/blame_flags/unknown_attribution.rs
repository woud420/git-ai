use super::{ExpectedLineExt, TestRepo, normalize_for_snapshot};

#[test]
fn test_blame_contents_from_stdin() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create initial file and commit
    file.set_contents(crate::lines!["Line 1", "Line 2".ai(), "Line 3", " "]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Now simulate uncommitted changes that would be passed via stdin
    // This is what an IDE would do - pass the buffer contents that haven't been saved yet
    let modified_content = "Changed\nLine 2\nLine 3\nLine 4 NEW\n";

    // Run git-ai blame with --contents - (read from stdin)
    let git_ai_output = repo
        .git_ai_with_stdin(
            &["blame", "--contents", "-", "test.txt"],
            modified_content.as_bytes(),
        )
        .unwrap();

    println!("\n[DEBUG] git-ai blame output:\n{}", git_ai_output);
    let lines = git_ai_output.lines().collect::<Vec<&str>>();

    assert!(
        lines[0].starts_with("00000000 (External file (--contents)"),
        "First line should be the  --contents"
    );

    assert!(
        lines[3].starts_with("00000000 (External file (--contents)"),
        "Last line should be the --contents"
    );
}

#[test]
fn test_blame_mark_unknown_without_authorship_log() {
    // Test that --mark-unknown shows "Unknown" for commits without authorship logs
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create a commit WITHOUT using git-ai (bypassing hooks to avoid authorship log)
    file.set_contents(crate::lines!["Line from untracked commit"]);

    // Use git_og to bypass git-ai hooks
    repo.git_og(&["add", "test.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "Commit without authorship log"])
        .unwrap();

    // Without --mark-unknown: should show author name
    let output_without_flag = repo.git_ai(&["blame", "test.txt"]).unwrap();
    println!("\n[DEBUG] Without --mark-unknown:\n{}", output_without_flag);
    assert!(
        output_without_flag.contains("Test User"),
        "Without flag, lines from untracked commits should show author name"
    );

    // With --mark-unknown: should show "Unknown"
    let output_with_flag = repo
        .git_ai(&["blame", "--mark-unknown", "test.txt"])
        .unwrap();
    println!("\n[DEBUG] With --mark-unknown:\n{}", output_with_flag);
    assert!(
        output_with_flag.contains("Unknown"),
        "With flag, lines from untracked commits should show 'Unknown'"
    );
    assert!(
        !output_with_flag.contains("Test User"),
        "With flag, should not show author name for untracked commits"
    );
}

#[test]
fn test_blame_mark_unknown_mixed_commits() {
    // Test a file with lines from both tracked and untracked commits
    // We'll create two separate files - one from untracked commit, one from tracked
    let repo = TestRepo::new();

    // Create file1 WITHOUT authorship log (using git_og)
    let file1_path = repo.path().join("untracked.txt");
    std::fs::write(&file1_path, "Untracked line\n").unwrap();
    repo.git_og(&["add", "untracked.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "Untracked commit"]).unwrap();

    // Create file2 WITH authorship log (through git-ai)
    let mut file2 = repo.filename("tracked.txt");
    file2.set_contents(crate::lines!["Tracked human line", "Tracked AI line".ai()]);
    repo.stage_all_and_commit("Tracked commit").unwrap();

    // Test untracked file with --mark-unknown
    let output1 = repo
        .git_ai(&["blame", "--mark-unknown", "untracked.txt"])
        .unwrap();
    println!("\n[DEBUG] Untracked file with --mark-unknown:\n{}", output1);
    assert!(
        output1.contains("Unknown"),
        "Untracked file should show Unknown: {}",
        output1
    );

    // Test tracked file with --mark-unknown
    let output2 = repo
        .git_ai(&["blame", "--mark-unknown", "tracked.txt"])
        .unwrap();
    println!("\n[DEBUG] Tracked file with --mark-unknown:\n{}", output2);

    let lines: Vec<&str> = output2.lines().collect();

    // Line 1 should show "Test User" (human line from tracked commit)
    assert!(
        lines[0].contains("Test User"),
        "Line 1 should show Test User: {}",
        lines[0]
    );

    // Line 2 should show AI tool name (AI line from tracked commit)
    assert!(
        lines[1].contains("mock_ai"),
        "Line 2 should show mock_ai: {}",
        lines[1]
    );
}

#[test]
fn test_blame_mark_unknown_backward_compatible() {
    // Ensure that without --mark-unknown, behavior matches git blame exactly
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create commit without authorship log (using git_og)
    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.git_og(&["add", "test.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "Untracked commit"]).unwrap();

    let git_output = repo.git(&["blame", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);

    println!("\n[DEBUG] git blame:\n{}", git_norm);
    println!("\n[DEBUG] git-ai blame:\n{}", git_ai_norm);

    assert_eq!(
        git_norm, git_ai_norm,
        "Without --mark-unknown, git-ai blame should match git blame exactly"
    );
}

crate::reuse_tests_in_worktree!(
    test_blame_contents_from_stdin,
    test_blame_mark_unknown_without_authorship_log,
    test_blame_mark_unknown_mixed_commits,
    test_blame_mark_unknown_backward_compatible,
);
