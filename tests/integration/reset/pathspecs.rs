use super::{ExpectedLineExt, TestRepo, fs};

/// Test git reset with pathspecs: should preserve AI authorship for non-reset files
#[test]
fn test_reset_with_pathspec() {
    let repo = TestRepo::new();
    let mut file1 = repo.filename("file1.txt");
    let mut file2 = repo.filename("file2.txt");

    // Create initial commit with multiple files
    file1.set_contents(crate::lines!["content 1", ""]);
    file2.set_contents(crate::lines!["content 2", ""]);
    let first_commit = repo.stage_all_and_commit("Initial").unwrap();

    // Commit AI changes to both files
    file1.insert_at(1, crate::lines!["// AI change 1".ai()]);
    file2.insert_at(1, crate::lines!["// AI change 2".ai()]);
    repo.stage_all_and_commit("AI changes both files").unwrap();

    // Make uncommitted changes to both files
    file1.insert_at(2, crate::lines!["// More AI".ai()]);
    file2.insert_at(2, crate::lines!["// More AI".ai()]);

    // Now reset only file1.txt to first commit with pathspec
    repo.git(&["reset", &first_commit.commit_sha, "--", "file1.txt"])
        .expect("reset with pathspec should succeed");

    // Stage all and commit to verify file2 still has AI attribution
    let new_commit = repo.stage_all_and_commit("After pathspec reset").unwrap();

    assert!(
        !new_commit.authorship_log.attestations.is_empty(),
        "AI authorship should be preserved for file2"
    );

    file2 = repo.filename("file2.txt");
    // file2 should still have AI changes
    file2.assert_lines_and_blame(crate::lines![
        "content 2".human(),
        "// AI change 2".ai(),
        "// More AI".ai(),
    ]);
}

/// Test git reset --mixed with pathspec: should preserve AI authorship for non-reset files
#[test]
fn test_reset_mixed_pathspec_preserves_ai_authorship() {
    let repo = TestRepo::new();
    let mut file1 = repo.filename("file1.txt");
    let mut file2 = repo.filename("file2.txt");

    // Base commit with two files
    file1.set_contents(crate::lines!["base content 1", ""]);
    file2.set_contents(crate::lines!["base content 2", ""]);
    let base_commit = repo.stage_all_and_commit("Base commit").unwrap();

    // Second commit: AI modifies both files
    file1.insert_at(1, crate::lines!["// AI change to file1".ai()]);
    file2.insert_at(1, crate::lines!["// AI change to file2".ai()]);
    let _second_commit = repo.stage_all_and_commit("AI modifies both files").unwrap();

    // Make uncommitted changes to file2 (not file1)
    file2.insert_at(2, crate::lines!["// More AI changes".ai()]);

    // Get current branch for HEAD check
    let current_head_before = repo.current_branch();

    // Reset only file1.txt to base commit with pathspec
    // This should preserve uncommitted changes for file2.txt
    repo.git(&["reset", &base_commit.commit_sha, "--", "file1.txt"])
        .expect("reset with pathspec should succeed");

    // HEAD should not move with pathspec reset
    let current_head_after = repo.current_branch();
    assert_eq!(
        current_head_before, current_head_after,
        "HEAD should not move with pathspec reset"
    );

    // Commit and verify file2 still has AI authorship
    let new_commit = repo.stage_all_and_commit("After pathspec reset").unwrap();

    assert!(
        !new_commit.authorship_log.attestations.is_empty(),
        "AI authorship should be preserved for file2 after pathspec reset"
    );

    file2 = repo.filename("file2.txt");
    file2.assert_lines_and_blame(crate::lines![
        "base content 2".human(),
        "// AI change to file2".ai(),
        "// More AI changes".ai(),
    ]);
}

/// Test git reset --mixed with pathspec on multiple commits worth of AI changes
#[test]
fn test_reset_mixed_pathspec_multiple_commits() {
    let repo = TestRepo::new();
    let mut app_file = repo.filename("app.js");
    let mut lib_file = repo.filename("lib.js");

    // Base commit
    app_file.set_contents(crate::lines!["// base", ""]);
    lib_file.set_contents(crate::lines!["// base", ""]);
    let base_commit = repo.stage_all_and_commit("Base").unwrap();

    // First AI commit - modifies both files
    app_file.insert_at(1, crate::lines!["// AI feature 1".ai()]);
    lib_file.insert_at(1, crate::lines!["// AI lib 1".ai()]);
    repo.stage_all_and_commit("AI feature 1").unwrap();

    // Second AI commit - modifies both files again
    app_file.insert_at(2, crate::lines!["// AI feature 2".ai()]);
    lib_file.insert_at(2, crate::lines!["// AI lib 2".ai()]);
    let _second_ai_commit = repo.stage_all_and_commit("AI feature 2").unwrap();

    // Make uncommitted changes to lib.js (not app.js)
    lib_file.insert_at(3, crate::lines!["// More lib".ai()]);

    // Get current branch for HEAD check
    let current_head_before = repo.current_branch();

    // Reset only app.js to base with pathspec
    // This should preserve uncommitted changes for lib.js
    repo.git(&["reset", &base_commit.commit_sha, "--", "app.js"])
        .expect("reset with pathspec should succeed");

    // HEAD should not move
    let current_head_after = repo.current_branch();
    assert_eq!(
        current_head_before, current_head_after,
        "HEAD should not move"
    );

    // Commit and verify lib.js retains AI authorship
    let new_commit = repo.stage_all_and_commit("After pathspec reset").unwrap();

    assert!(
        !new_commit.authorship_log.attestations.is_empty(),
        "AI authorship should be preserved for lib.js after pathspec reset"
    );

    lib_file = repo.filename("lib.js");
    lib_file.assert_lines_and_blame(crate::lines![
        "// base".human(),
        "// AI lib 1".ai(),
        "// AI lib 2".ai(),
        "// More lib".ai(),
    ]);
}

/// Test git reset with directory pathspec: should reset only files in the specified directory
#[test]
fn test_reset_with_directory_pathspec() {
    let repo = TestRepo::new();

    // Create directory structure
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::create_dir_all(repo.path().join("lib")).unwrap();

    let mut src_file = repo.filename("src/app.rs");
    let mut lib_file = repo.filename("lib/utils.rs");
    let mut root_file = repo.filename("root.txt");

    // Base commit with files in different directories
    src_file.set_contents(crate::lines!["fn main() {}", ""]);
    lib_file.set_contents(crate::lines!["pub fn helper() {}", ""]);
    root_file.set_contents(crate::lines!["root content", ""]);
    let base_commit = repo.stage_all_and_commit("Base commit").unwrap();

    // Second commit: AI modifies files in all directories
    src_file.insert_at(1, crate::lines!["    // AI src change".ai()]);
    lib_file.insert_at(1, crate::lines!["    // AI lib change".ai()]);
    root_file.insert_at(1, crate::lines!["// AI root change".ai()]);
    repo.stage_all_and_commit("AI changes everywhere").unwrap();

    // Make uncommitted AI changes to lib and root (not src)
    lib_file.insert_at(2, crate::lines!["    // More AI lib".ai()]);
    root_file.insert_at(2, crate::lines!["// More AI root".ai()]);

    // Reset only the src directory to base commit using directory pathspec
    repo.git(&["reset", &base_commit.commit_sha, "--", "src"])
        .expect("reset with directory pathspec should succeed");

    // Stage all and commit to verify attributions
    let new_commit = repo
        .stage_all_and_commit("After directory pathspec reset")
        .unwrap();

    assert!(
        !new_commit.authorship_log.attestations.is_empty(),
        "AI authorship should be preserved for lib and root files"
    );

    // lib/utils.rs should still have AI changes (not in reset pathspec)
    lib_file = repo.filename("lib/utils.rs");
    lib_file.assert_lines_and_blame(crate::lines![
        "pub fn helper() {}".human(),
        "    // AI lib change".ai(),
        "    // More AI lib".ai(),
    ]);

    // root.txt should still have AI changes (not in reset pathspec)
    root_file = repo.filename("root.txt");
    root_file.assert_lines_and_blame(crate::lines![
        "root content".human(),
        "// AI root change".ai(),
        "// More AI root".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_reset_with_pathspec,
    test_reset_mixed_pathspec_preserves_ai_authorship,
    test_reset_mixed_pathspec_multiple_commits,
    test_reset_with_directory_pathspec,
);
