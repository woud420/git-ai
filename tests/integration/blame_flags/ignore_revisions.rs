use super::{TestRepo, normalize_for_snapshot};

// =============================================================================
// Tests for .git-blame-ignore-revs auto-detection (Issue #363)
// =============================================================================

#[test]
fn test_blame_auto_detects_git_blame_ignore_revs_file() {
    // Test that git-ai blame automatically uses .git-blame-ignore-revs when present
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create initial commit with some content
    file.set_contents(crate::lines!["Line 1", "Line 2", "Line 3"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Get the initial commit SHA
    let initial_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create a "formatting" commit that we want to ignore
    file.set_contents(crate::lines!["  Line 1", "  Line 2", "  Line 3"]); // Add indentation
    repo.stage_all_and_commit("Format: add indentation")
        .unwrap();

    // Get the formatting commit SHA
    let format_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create .git-blame-ignore-revs file with the formatting commit
    let ignore_revs_path = repo.path().join(".git-blame-ignore-revs");
    std::fs::write(
        &ignore_revs_path,
        format!("# Formatting commit\n{}\n", format_sha),
    )
    .unwrap();

    // Run git-ai blame - it should auto-detect .git-blame-ignore-revs
    let git_ai_output = repo.git_ai(&["blame", "test.txt"]).unwrap();
    println!(
        "\n[DEBUG] git-ai blame with auto-detected ignore-revs:\n{}",
        git_ai_output
    );

    // The blame should show the initial commit, not the formatting commit
    // (because the formatting commit is in .git-blame-ignore-revs)
    assert!(
        git_ai_output.contains(&initial_sha[..7]),
        "Blame should show initial commit SHA when formatting commit is ignored. Output: {}",
        git_ai_output
    );
}

#[test]
fn test_blame_no_ignore_revs_file_flag_disables_auto_detection() {
    // Test that --no-ignore-revs-file disables auto-detection
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create initial commit
    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Create a second commit
    file.set_contents(crate::lines!["Modified Line 1", "Line 2"]);
    repo.stage_all_and_commit("Modify line 1").unwrap();

    // Get the second commit SHA
    let second_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create .git-blame-ignore-revs to ignore the second commit
    let ignore_revs_path = repo.path().join(".git-blame-ignore-revs");
    std::fs::write(&ignore_revs_path, format!("{}\n", second_sha)).unwrap();

    // Run with --no-ignore-revs-file - should NOT use .git-blame-ignore-revs
    let output_without_ignore = repo
        .git_ai(&["blame", "--no-ignore-revs-file", "test.txt"])
        .unwrap();
    println!(
        "\n[DEBUG] git-ai blame with --no-ignore-revs-file:\n{}",
        output_without_ignore
    );

    // The second commit should appear in the output (not ignored)
    assert!(
        output_without_ignore.contains(&second_sha[..7]),
        "With --no-ignore-revs-file, the second commit should appear in blame. Output: {}",
        output_without_ignore
    );
}

#[test]
fn test_blame_explicit_ignore_revs_file_takes_precedence() {
    // Test that explicit --ignore-revs-file takes precedence over auto-detection
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create initial commit
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let _initial_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create second commit
    file.set_contents(crate::lines!["Line 1 modified"]);
    repo.stage_all_and_commit("Second commit").unwrap();

    let second_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create third commit
    file.set_contents(crate::lines!["Line 1 modified again"]);
    repo.stage_all_and_commit("Third commit").unwrap();

    let third_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create .git-blame-ignore-revs that ignores the SECOND commit
    let default_ignore_path = repo.path().join(".git-blame-ignore-revs");
    std::fs::write(&default_ignore_path, format!("{}\n", second_sha)).unwrap();

    // Create a custom ignore file that ignores the THIRD commit
    let custom_ignore_path = repo.path().join("custom-ignore-revs");
    std::fs::write(&custom_ignore_path, format!("{}\n", third_sha)).unwrap();

    // Run with explicit --ignore-revs-file pointing to custom file
    let output = repo
        .git_ai(&[
            "blame",
            "--ignore-revs-file",
            "custom-ignore-revs",
            "test.txt",
        ])
        .unwrap();
    println!(
        "\n[DEBUG] git-ai blame with explicit --ignore-revs-file:\n{}",
        output
    );

    // The second commit (ignored by default file) should appear
    // because we're using the custom file which ignores the third commit
    assert!(
        output.contains(&second_sha[..7]),
        "Explicit --ignore-revs-file should take precedence. Second commit should appear. Output: {}",
        output
    );
}

#[test]
fn test_blame_respects_git_config_blame_ignore_revs_file() {
    // Test that git-ai respects the blame.ignoreRevsFile git config
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create initial commit
    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let initial_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create a commit we want to ignore
    file.set_contents(crate::lines!["Line 1 reformatted", "Line 2 reformatted"]);
    repo.stage_all_and_commit("Reformat code").unwrap();

    let reformat_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create ignore file with a custom name (not .git-blame-ignore-revs)
    let custom_ignore_path = repo.path().join("my-ignore-revs");
    std::fs::write(&custom_ignore_path, format!("{}\n", reformat_sha)).unwrap();

    // Set git config to point to the custom file
    repo.git_og(&["config", "blame.ignoreRevsFile", "my-ignore-revs"])
        .unwrap();

    // Run git-ai blame - should auto-detect from git config
    let output = repo.git_ai(&["blame", "test.txt"]).unwrap();
    println!(
        "\n[DEBUG] git-ai blame with blame.ignoreRevsFile config:\n{}",
        output
    );

    // The initial commit should appear (reformat commit should be ignored via config)
    assert!(
        output.contains(&initial_sha[..7]),
        "Should respect blame.ignoreRevsFile config. Initial commit should appear. Output: {}",
        output
    );
}

#[test]
fn test_blame_without_ignore_revs_file_works_normally() {
    // Test that blame works normally when no .git-blame-ignore-revs exists
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create initial commit
    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Create second commit
    file.set_contents(crate::lines!["Line 1 modified"]);
    repo.stage_all_and_commit("Second commit").unwrap();

    let second_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // No .git-blame-ignore-revs file exists

    // Run git-ai blame - should work normally
    let git_ai_output = repo.git_ai(&["blame", "test.txt"]).unwrap();
    let git_output = repo.git(&["blame", "test.txt"]).unwrap();

    println!("\n[DEBUG] git blame:\n{}", git_output);
    println!("\n[DEBUG] git-ai blame:\n{}", git_ai_output);

    // Both should show the second commit
    assert!(
        git_ai_output.contains(&second_sha[..7]),
        "git-ai blame should show second commit when no ignore file exists. Output: {}",
        git_ai_output
    );

    // Outputs should match (normalized)
    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    assert_eq!(
        git_norm, git_ai_norm,
        "Blame outputs should match when no ignore file exists"
    );
}

#[test]
fn test_blame_ignore_revs_with_multiple_commits() {
    // Test ignoring multiple commits in .git-blame-ignore-revs
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create initial commit
    file.set_contents(crate::lines!["original"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let initial_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create first formatting commit
    file.set_contents(crate::lines!["  original"]);
    repo.stage_all_and_commit("Format 1: add spaces").unwrap();

    let format1_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create second formatting commit
    file.set_contents(crate::lines!["    original"]);
    repo.stage_all_and_commit("Format 2: add more spaces")
        .unwrap();

    let format2_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Create .git-blame-ignore-revs with both formatting commits
    let ignore_revs_path = repo.path().join(".git-blame-ignore-revs");
    std::fs::write(
        &ignore_revs_path,
        format!(
            "# Formatting commits to ignore\n{}\n{}\n",
            format1_sha, format2_sha
        ),
    )
    .unwrap();

    // Run git-ai blame
    let output = repo.git_ai(&["blame", "test.txt"]).unwrap();
    println!(
        "\n[DEBUG] git-ai blame with multiple ignored commits:\n{}",
        output
    );

    // Should show the initial commit (both formatting commits are ignored)
    assert!(
        output.contains(&initial_sha[..7]),
        "Both formatting commits should be ignored. Initial commit should appear. Output: {}",
        output
    );
}

crate::reuse_tests_in_worktree!(
    test_blame_auto_detects_git_blame_ignore_revs_file,
    test_blame_no_ignore_revs_file_flag_disables_auto_detection,
    test_blame_explicit_ignore_revs_file_takes_precedence,
    test_blame_respects_git_config_blame_ignore_revs_file,
    test_blame_without_ignore_revs_file_works_normally,
    test_blame_ignore_revs_with_multiple_commits,
);
