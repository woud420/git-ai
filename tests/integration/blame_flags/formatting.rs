use super::{ExpectedLineExt, TestRepo, normalize_for_snapshot};

#[test]
fn test_blame_basic_format() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1",
        "Line 2",
        "Line 3".ai(),
        "Line 4".ai()
    ]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // Run git blame and git-ai blame
    let git_output = repo.git(&["blame", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "test.txt"]).unwrap();

    // Compare normalized outputs
    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );
}

#[test]
fn test_blame_porcelain_format() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo.git(&["blame", "--porcelain", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "--porcelain", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );
}

#[test]
fn test_blame_long_rev() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo.git(&["blame", "-l", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "-l", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );

    // Verify both show long revision hashes
    let git_sha_len = git_output
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .len();
    let git_ai_sha_len = git_ai_output
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .len();

    assert!(git_sha_len > 8, "Git should show long revision");
    assert!(git_ai_sha_len > 8, "Git-ai should show long revision");
}

#[test]
fn test_blame_raw_timestamp() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo.git(&["blame", "-t", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "-t", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );

    // Verify both contain raw timestamps (Unix timestamps)
    assert!(
        git_output.chars().any(|c| c.is_numeric()),
        "Git output should contain timestamps"
    );
    assert!(
        git_ai_output.chars().any(|c| c.is_numeric()),
        "Git-ai output should contain timestamps"
    );
}

#[test]
fn test_blame_abbrev() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // Note: git requires --abbrev=4 format, git-ai accepts --abbrev 4
    let git_output = repo.git(&["blame", "--abbrev=4", "test.txt"]).unwrap();
    let git_ai_output = repo
        .git_ai(&["blame", "--abbrev", "4", "test.txt"])
        .unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );
}

#[test]
fn test_blame_blank_boundary() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo.git(&["blame", "-b", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "-b", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );
}

#[test]
fn test_blame_show_root() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo.git(&["blame", "--root", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "--root", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );

    // Verify both handle root commits
    assert!(
        git_output.lines().count() > 0,
        "Git should handle root commits"
    );
    assert!(
        git_ai_output.lines().count() > 0,
        "Git-ai should handle root commits"
    );
}

// #[test]
// fn test_blame_show_stats() {
//     let tmp_dir = tempdir().unwrap();
//     let repo_path = tmp_dir.path().to_path_buf();

//     let tmp_repo = TmpRepo::new().unwrap();
//     let mut file = tmp_repo.write_file("test.txt", "Line 1\n", true).unwrap();

//     tmp_repo
//         .trigger_checkpoint_with_author("test_user")
//         .unwrap();
//     file.append("Line 2\n").unwrap();
//     tmp_repo.trigger_checkpoint_with_ai("Claude", Some("claude-3-sonnet"), Some("cursor")).unwrap();
//     tmp_repo.commit_with_message("Initial commit").unwrap();

//     let git_output = run_git_blame(tmp_repo.path(), "test.txt", &["--show-stats"]);
//     let git_ai_output = run_git_ai_blame(tmp_repo.path(), "test.txt", &["--show-stats"]);

//     let _comparison = create_blame_comparison(&git_output, &git_ai_output, "show_stats");
//     let git_norm = normalize_for_snapshot(&git_output);
//     let git_ai_norm = normalize_for_snapshot(&git_ai_output);
//     println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
//     println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
//     assert_eq!(
//         git_norm, git_ai_norm,
//         "Normalized blame outputs should match exactly"
//     );

//     // Verify both show statistics
//     assert!(
//         git_output.contains("%"),
//         "Git output should contain statistics"
//     );
//     assert!(
//         git_ai_output.contains("%"),
//         "Git-ai output should contain statistics"
//     );
// }
#[test]
fn test_blame_date_format() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // Note: git requires --date=short format, git-ai accepts --date short
    let git_output = repo.git(&["blame", "--date=short", "test.txt"]).unwrap();
    let git_ai_output = repo
        .git_ai(&["blame", "--date", "short", "test.txt"])
        .unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );

    // Verify both use short date format
    assert!(git_output.contains("-"), "Git output should contain date");
    assert!(
        git_ai_output.contains("-"),
        "Git-ai output should contain date"
    );
}

#[test]
fn test_blame_multiple_flags() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1",
        "Line 2",
        "Line 3",
        "Line 4".ai(),
        "Line 5".ai()
    ]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // Test multiple flags together
    let git_output = repo
        .git(&["blame", "-L", "2,4", "-e", "-n", "test.txt"])
        .unwrap();
    let git_ai_output = repo
        .git_ai(&["blame", "-L", "2,4", "-e", "-n", "test.txt"])
        .unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );

    // Verify both handle multiple flags
    assert!(
        git_output.lines().count() > 0,
        "Git should handle multiple flags"
    );
    assert!(
        git_ai_output.lines().count() > 0,
        "Git-ai should handle multiple flags"
    );

    // Verify both contain email and line numbers
    assert!(git_output.contains("@"), "Git output should contain email");
    assert!(
        git_ai_output.contains("@"),
        "Git-ai output should contain email"
    );
}

#[test]
fn test_blame_incremental_format() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo.git(&["blame", "--incremental", "test.txt"]).unwrap();
    let git_ai_output = repo
        .git_ai(&["blame", "--incremental", "test.txt"])
        .unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );
}

#[test]
fn test_blame_line_porcelain() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo
        .git(&["blame", "--line-porcelain", "test.txt"])
        .unwrap();
    let git_ai_output = repo
        .git_ai(&["blame", "--line-porcelain", "test.txt"])
        .unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );
}

crate::reuse_tests_in_worktree!(
    test_blame_basic_format,
    test_blame_porcelain_format,
    test_blame_long_rev,
    test_blame_raw_timestamp,
    test_blame_abbrev,
    test_blame_blank_boundary,
    test_blame_show_root,
    test_blame_date_format,
    test_blame_multiple_flags,
    test_blame_incremental_format,
    test_blame_line_porcelain,
);
