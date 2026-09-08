use super::{
    ExpectedLineExt, TestRepo, assert_multiple_range_blame_matches_git, normalize_for_snapshot,
};

#[test]
fn test_blame_line_range() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1",
        "Line 2",
        "Line 3",
        "Line 4",
        "Line 5".ai(),
        "Line 6".ai()
    ]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // Test -L flag
    let git_output = repo.git(&["blame", "-L", "2,4", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "-L", "2,4", "test.txt"]).unwrap();

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
fn test_blame_multiple_line_ranges_default() {
    assert_multiple_range_blame_matches_git(
        &["blame", "-L", "2,3", "-L", "6,8", "test.txt"],
        "Normalized blame outputs should match exactly for multiple -L ranges",
    );
}

#[test]
fn test_blame_multiple_line_ranges_default_reversed_order() {
    assert_multiple_range_blame_matches_git(
        &["blame", "-L", "6,8", "-L", "2,3", "test.txt"],
        "Normalized blame outputs should match exactly when -L ranges are specified out of order",
    );
}

#[test]
fn test_blame_multiple_line_ranges_overlap_default() {
    assert_multiple_range_blame_matches_git(
        &["blame", "-L", "2,5", "-L", "4,7", "test.txt"],
        "Normalized blame outputs should match exactly for overlapping -L ranges",
    );
}

#[test]
fn test_blame_multiple_line_ranges_porcelain() {
    assert_multiple_range_blame_matches_git(
        &["blame", "--porcelain", "-L", "2,3", "-L", "6,8", "test.txt"],
        "Porcelain output should match exactly for multiple -L ranges",
    );
}

#[test]
fn test_blame_multiple_line_ranges_line_porcelain() {
    assert_multiple_range_blame_matches_git(
        &[
            "blame",
            "--line-porcelain",
            "-L",
            "2,3",
            "-L",
            "6,8",
            "test.txt",
        ],
        "Line porcelain output should match exactly for multiple -L ranges",
    );
}

#[test]
fn test_blame_multiple_line_ranges_incremental() {
    assert_multiple_range_blame_matches_git(
        &[
            "blame",
            "--incremental",
            "-L",
            "2,3",
            "-L",
            "6,8",
            "test.txt",
        ],
        "Incremental output should match exactly for multiple -L ranges",
    );
}

#[test]
fn test_blame_duplicate_line_ranges_match_git() {
    assert_multiple_range_blame_matches_git(
        &["blame", "-L", "2,3", "-L", "2,3", "test.txt"],
        "Normalized blame output should match exactly for duplicate -L ranges",
    );
}

#[test]
fn test_blame_machine_format_output_matches_git_byte_for_byte() {
    let repo = TestRepo::new();
    repo.filename("test.txt")
        .set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    for format in ["--porcelain", "--line-porcelain", "--incremental"] {
        let args = ["blame", format, "test.txt"];
        assert_eq!(repo.git(&args).unwrap(), repo.git_ai(&args).unwrap());
    }
}

#[test]
fn test_blame_porcelain_multiple_hunks_same_commit_matches_git_filename_behavior() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1", "Line 2", "Line 3", "Line 4", "Line 5"
    ]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    file.set_contents(crate::lines![
        "Line 1",
        "Line 2",
        "Changed line 3",
        "Line 4",
        "Line 5"
    ]);
    repo.stage_all_and_commit("Change middle line").unwrap();

    let args = ["blame", "--porcelain", "-L", "1,2", "-L", "4,5", "test.txt"];
    let git_output = repo.git(&args).unwrap();
    let git_ai_output = repo.git_ai(&args).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    assert_eq!(
        git_norm, git_ai_norm,
        "Porcelain output should match exactly when the same commit appears in multiple hunks"
    );

    let git_filename_count = git_output
        .lines()
        .filter(|line| line.starts_with("filename "))
        .count();
    let git_ai_filename_count = git_ai_output
        .lines()
        .filter(|line| line.starts_with("filename "))
        .count();
    assert_eq!(
        git_filename_count, git_ai_filename_count,
        "git-ai should emit filename lines in the same places as git"
    );
    assert_eq!(
        1, git_ai_filename_count,
        "When git has already emitted commit metadata, subsequent hunks from the same commit do not repeat filename"
    );
}

#[test]
fn test_blame_incremental_uses_real_commit_summaries() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    file.set_contents(crate::lines!["Line 1", "Updated line 2"]);
    repo.stage_all_and_commit("Update second line").unwrap();

    let args = ["blame", "--incremental", "-L", "1,2", "test.txt"];
    let git_output = repo.git(&args).unwrap();
    let git_ai_output = repo.git_ai(&args).unwrap();

    let mut git_summaries: Vec<&str> = git_output
        .lines()
        .filter(|line| line.starts_with("summary "))
        .collect();
    let mut git_ai_summaries: Vec<&str> = git_ai_output
        .lines()
        .filter(|line| line.starts_with("summary "))
        .collect();
    git_summaries.sort_unstable();
    git_ai_summaries.sort_unstable();
    assert_eq!(
        git_summaries, git_ai_summaries,
        "git-ai incremental output should report the same commit summaries as git"
    );

    assert!(
        git_output.contains("summary Update second line"),
        "Sanity check: git incremental output should contain the newer commit summary"
    );
    assert!(
        git_ai_output.contains("summary Update second line"),
        "git-ai incremental output should include real commit summaries"
    );
}

crate::reuse_tests_in_worktree!(test_blame_line_range,);
