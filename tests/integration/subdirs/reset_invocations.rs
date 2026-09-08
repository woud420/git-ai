use super::{ExpectedLineExt, fs};

crate::subdir_test_variants! {
    fn reset_hard() {
        // Test git reset --hard: should discard all changes and reset to target commit
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src").join("lib");
    fs::create_dir_all(&working_dir).unwrap();

    let mut file = repo.filename("test.txt");

    // Create initial commit
    file.set_contents(crate::lines!["line 1", "line 2", "line 3"]);
    let first_commit = repo.stage_all_and_commit("First commit").unwrap();

    // Make second commit with AI changes
    file.insert_at(3, crate::lines!["// AI line".ai()]);
    repo.stage_all_and_commit("Second commit").unwrap();

    // Make some uncommitted AI changes
    file.insert_at(4, crate::lines!["// Uncommitted".ai()]);

    // Reset --hard to first commit
    repo.git_from_working_dir(&working_dir, &["reset", "--hard", &first_commit.commit_sha])
        .expect("reset --hard should succeed");

    // After hard reset, file should match first commit (no AI lines, no uncommitted changes)
    file = repo.filename("test.txt");
    file.assert_lines_and_blame(crate::lines!["line 1", "line 2", "line 3"]);

    // Make a new commit to verify working directory is clean
    file.insert_at(3, crate::lines!["new line"]);
    repo.stage_all_and_commit("After reset").unwrap();
    file = repo.filename("test.txt");
    file.assert_lines_and_blame(crate::lines!["line 1", "line 2", "line 3", "new line",]);
    }
}

crate::subdir_test_variants! {
    fn reset_soft() {
        // Test git reset --soft: should preserve AI authorship from unwound commits
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src");
    fs::create_dir_all(&working_dir).unwrap();

    let mut file = repo.filename("test.txt");

    // Create initial commit
    file.set_contents(crate::lines!["line 1", "line 2"]);
    let first_commit = repo.stage_all_and_commit("First commit").unwrap();

    // Make second commit with AI changes
    file.insert_at(2, crate::lines!["// AI addition".ai()]);
    repo.stage_all_and_commit("Second commit").unwrap();

    // Reset --soft to first commit
    repo.git_from_working_dir(&working_dir, &["reset", "--soft", &first_commit.commit_sha])
        .expect("reset --soft should succeed");

    // After soft reset, changes should be staged, and when we commit them
    // they should retain AI authorship
    let new_commit = repo.commit("Re-commit AI changes").unwrap();

    // Verify AI authorship was preserved in the commit
    assert!(
        !new_commit.authorship_log.attestations.is_empty(),
        "AI authorship should be preserved after reset --soft"
    );

    // Verify blame shows AI authorship
    file = repo.filename("test.txt");
    file.assert_lines_and_blame(crate::lines![
        "line 1".human(),
        "line 2".ai(),
        "// AI addition".ai(),
    ]);
    }
}

crate::subdir_test_variants! {
    fn reset_mixed() {
        // Test git reset --mixed (default): working directory preserved
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src").join("lib");
    fs::create_dir_all(&working_dir).unwrap();

    let mut file = repo.filename("main.rs");

    // Create initial commit
    file.set_contents(crate::lines!["fn main() {", "}"]);
    let first_commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // Make second commit with AI changes
    file.insert_at(1, crate::lines!["    // AI: Added logging".ai()]);
    file.insert_at(2, crate::lines!["    println!(\"Hello\");".ai()]);

    repo.stage_all_and_commit("Add logging").unwrap();

    // Reset --mixed to first commit
    repo.git_from_working_dir(&working_dir, &["reset", "--mixed", &first_commit.commit_sha])
        .expect("reset --mixed should succeed");

    // After mixed reset, changes should be unstaged but in working directory
    // Stage and commit them to verify AI authorship was preserved
    let new_commit = repo.stage_all_and_commit("Re-commit after reset").unwrap();

    // Verify AI authorship was preserved
    assert!(
        !new_commit.authorship_log.attestations.is_empty(),
        "AI authorship should be preserved after reset --mixed"
    );

    file = repo.filename("main.rs");
    file.assert_lines_and_blame(crate::lines![
        "fn main() {".human(),
        "    // AI: Added logging".ai(),
        "    println!(\"Hello\");".ai(),
        "}".human(),
    ]);
    }
}

crate::subdir_test_variants! {
    fn reset_multiple_commits() {
        // Test git reset with multiple commits unwound: should preserve all AI authorship
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("lib");
    fs::create_dir_all(&working_dir).unwrap();

    let mut file = repo.filename("code.js");

    // Create base commit
    file.set_contents(crate::lines!["// Base", ""]);
    let base_commit = repo.stage_all_and_commit("Base").unwrap();

    // Second commit - AI adds feature
    file.insert_at(1, crate::lines!["// AI feature 1".ai()]);
    repo.stage_all_and_commit("Feature 1").unwrap();

    // Third commit - AI adds another feature
    file.insert_at(2, crate::lines!["// AI feature 2".ai()]);
    repo.stage_all_and_commit("Feature 2").unwrap();

    // Reset --soft to base
    repo.git_from_working_dir(&working_dir, &["reset", "--soft", &base_commit.commit_sha])
        .expect("reset --soft should succeed");

    // Commit and verify both AI features are attributed correctly
    let new_commit = repo.commit("Re-commit features").unwrap();

    assert!(
        !new_commit.authorship_log.attestations.is_empty(),
        "AI authorship should be preserved for all unwound commits"
    );

    file = repo.filename("code.js");
    file.assert_lines_and_blame(crate::lines![
        "// Base".human(),
        "// AI feature 1".ai(),
        "// AI feature 2".ai(),
    ]);
    }
}

crate::subdir_test_variants! {
    fn reset_with_pathspec() {
        // Test git reset with pathspecs: should preserve AI authorship for non-reset files
    let repo = TestRepo::new();

    // Create subdirectory structure
    let working_dir = repo.path().join("src").join("components");
    fs::create_dir_all(&working_dir).unwrap();

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
    repo.git_from_working_dir(&working_dir, &["reset", &first_commit.commit_sha, "--", "file1.txt"])
        .expect("reset with pathspec should succeed");

    // Stage all and commit to verify file2 still has AI attribution
    let new_commit = repo.stage_all_and_commit("After pathspec reset").unwrap();

    assert!(
        !new_commit.authorship_log.attestations.is_empty(),
        "AI authorship should be preserved for file2 after pathspec reset"
    );

    file2 = repo.filename("file2.txt");
    // file2 should still have AI changes
    file2.assert_lines_and_blame(crate::lines![
        "content 2".human(),
        "// AI change 2".ai(),
        "// More AI".ai(),
    ]);
    }
}

crate::subdir_test_variants! {
    fn reset_mixed_ai_human_changes() {
        // Test git reset with AI and human mixed changes: should preserve all authorship
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("main.rs");

        // Base commit has known-human wrapper context.
        file.set_contents(crate::lines!["fn main() {", "}"]);
        let base = repo.stage_all_and_commit("Base").unwrap();

        // AI commit
        file.insert_at(1, crate::lines!["    // AI".ai()]);
        repo.stage_all_and_commit("AI changes").unwrap();

        // Human commit
        file.insert_at(2, crate::lines!["    // Human"]);
        repo.stage_all_and_commit("Human changes").unwrap();

        // Reset to base
        repo.git_from_working_dir(&working_dir, &["reset", "--soft", &base.commit_sha])
            .expect("reset --soft should succeed");

        // Commit and verify authorship
        let new_commit = repo.commit("Re-commit mixed changes").unwrap();

        assert!(
            !new_commit.authorship_log.attestations.is_empty(),
            "AI authorship should be preserved in mixed AI/human changes"
        );

        file = repo.filename("main.rs");
        file.assert_lines_and_blame(crate::lines![
            "fn main() {".human(),
            "    // AI".ai(),
            "    // Human".human(),
            "}".human(),
        ]);
    }
}

crate::subdir_test_variants! {
    fn reset_with_new_files() {
        // Test git reset with new files added in unwound commit: should preserve AI authorship
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        let mut old_file = repo.filename("old.txt");

        // Base commit
        old_file.set_contents(crate::lines!["existing"]);
        let base = repo.stage_all_and_commit("Base").unwrap();

        // Add new file in second commit
        let mut new_file = repo.filename("new.txt");
        new_file.set_contents(crate::lines!["// AI created this".ai()]);
        repo.stage_all_and_commit("Add new file").unwrap();

        // Reset to base
        repo.git_from_working_dir(&working_dir, &["reset", "--soft", &base.commit_sha])
            .expect("reset --soft should succeed");

        // Commit and verify new file has AI authorship
        let new_commit = repo.commit("Re-commit with new file").unwrap();

        assert!(
            !new_commit.authorship_log.attestations.is_empty(),
            "AI authorship should be preserved for new file after reset"
        );

        new_file = repo.filename("new.txt");
        new_file.assert_lines_and_blame(crate::lines!["// AI created this".ai()]);
    }
}

crate::subdir_test_variants! {
    fn reset_nested() {
        // Test git reset when run from a deeply nested subdirectory
        let repo = TestRepo::new();

        // Create deeply nested subdirectory structure
        let working_dir = repo.path().join("a").join("b").join("c");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("test.txt");

        // Create base commit
        file.set_contents(crate::lines!["base content"]);
        let base_commit = repo.stage_all_and_commit("Base").unwrap();

        // Second commit with AI changes
        file.insert_at(1, crate::lines!["// AI feature".ai()]);
        repo.stage_all_and_commit("AI feature").unwrap();

        // Reset --soft to base
        repo.git_from_working_dir(&working_dir, &["reset", "--soft", &base_commit.commit_sha])
            .expect("reset --soft should succeed");

        // Commit and verify AI authorship preserved
        let new_commit = repo.commit("Re-commit after reset").unwrap();

        assert!(
            !new_commit.authorship_log.attestations.is_empty(),
            "AI authorship should be preserved after reset"
        );

        file = repo.filename("test.txt");
        file.assert_lines_and_blame(crate::lines![
            "base content".ai(),
            "// AI feature".ai(),
        ]);
    }
}

crate::subdir_test_variants! {
    fn reset_to_same_commit() {
        // Test git reset to same commit: should preserve uncommitted AI changes
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("test.txt");

        // Create commit with AI changes
        file.set_contents(crate::lines!["line 1", "// AI line".ai(), ""]);
        repo.stage_all_and_commit("Commit").unwrap();

        // Make uncommitted changes
        file.insert_at(2, crate::lines!["// More changes".ai()]);

        // Reset to same commit (HEAD)
        repo.git_from_working_dir(&working_dir, &["reset", "HEAD"])
            .expect("reset should succeed");

        // Uncommitted AI changes should still be preserved in working directory
        // Commit them to verify authorship
        let new_commit = repo.stage_all_and_commit("After reset to HEAD").unwrap();

        assert!(
            !new_commit.authorship_log.attestations.is_empty(),
            "AI authorship should be preserved for uncommitted changes after reset"
        );

        file = repo.filename("test.txt");
        file.assert_lines_and_blame(crate::lines![
            "line 1".human(),
            "// AI line".ai(),
            "// More changes".ai(),
        ]);
    }
}

crate::subdir_test_variants! {
    fn reset_forward() {
        // Test git reset forward (to descendant): should restore commit state
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("lib");
        fs::create_dir_all(&working_dir).unwrap();

        let mut file = repo.filename("test.txt");

        // Create two commits
        file.set_contents(crate::lines!["v1"]);
        let first_commit = repo.stage_all_and_commit("First").unwrap();

        file.insert_at(1, crate::lines!["v2".ai()]);
        let second_commit = repo.stage_all_and_commit("Second").unwrap();

        // Reset back to first (--hard discards all changes)
        repo.git_from_working_dir(&working_dir, &["reset", "--hard", &first_commit.commit_sha])
            .expect("reset --hard should succeed");

        // Verify file is back to v1 only
        file = repo.filename("test.txt");
        file.assert_lines_and_blame(crate::lines!["v1".human()]);

        // Now reset forward to second with --hard to restore the working tree
        repo.git_from_working_dir(&working_dir, &["reset", "--hard", &second_commit.commit_sha])
            .expect("reset --hard forward should succeed");

        // File should now match second commit
        file = repo.filename("test.txt");
        file.assert_lines_and_blame(crate::lines!["v1".ai(), "v2".ai()]);
    }
}

crate::subdir_test_variants! {
    fn reset_mixed_pathspec() {
        // Test git reset --mixed with pathspec: should preserve AI authorship for non-reset files
        let repo = TestRepo::new();

        // Create subdirectory structure
        let working_dir = repo.path().join("src").join("components");
        fs::create_dir_all(&working_dir).unwrap();

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
        repo.git_from_working_dir(&working_dir, &["reset", &base_commit.commit_sha, "--", "file1.txt"])
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
}
