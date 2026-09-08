use super::{ExpectedLineExt, GitAiBlameOptions, GitAiRepository, TestRepo};

// =============================================================================
// Output Format Tests - Porcelain, incremental, JSON formats
// =============================================================================

#[test]
fn test_blame_format_porcelain_basic() {
    // Output format: Basic porcelain format
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);
    repo.stage_all_and_commit("Test").unwrap();

    let output = repo.git_ai(&["blame", "--porcelain", "test.txt"]).unwrap();

    // Porcelain format should include metadata fields
    assert!(output.contains("author "));
    assert!(output.contains("author-mail "));
    assert!(output.contains("author-time "));
    assert!(output.contains("committer "));
    assert!(output.contains("summary "));
    assert!(output.contains("filename "));
    assert!(output.contains("\tLine 1"));
    assert!(output.contains("\tLine 2"));
}

#[test]
fn test_blame_format_line_porcelain() {
    // Output format: Line porcelain format (metadata for every line)
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.stage_all_and_commit("Test").unwrap();

    let output = repo
        .git_ai(&["blame", "--line-porcelain", "test.txt"])
        .unwrap();

    // Line porcelain should have metadata for each line
    let author_count = output.matches("author ").count();
    assert!(author_count >= 2, "Should have author for each line");
}

#[test]
fn test_blame_format_incremental() {
    // Output format: Incremental format
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    repo.stage_all_and_commit("Test").unwrap();

    let output = repo
        .git_ai(&["blame", "--incremental", "test.txt"])
        .unwrap();

    // Incremental format should have metadata without content lines
    assert!(output.contains("author "));
    assert!(output.contains("filename "));
    assert!(!output.contains("\tLine 1")); // No content lines in incremental
}

#[test]
fn test_blame_format_json_structure() {
    // Output format: JSON format structure validation
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);
    repo.stage_all_and_commit("Test").unwrap();

    let output = repo.git_ai(&["blame", "--json", "test.txt"]).unwrap();

    let json: serde_json::Value = serde_json::from_str(&output).expect("Should be valid JSON");

    // Verify JSON structure matches JsonBlameOutput
    assert!(json.get("lines").is_some());
    assert!(json.get("prompts").is_some());

    let lines = json["lines"].as_object().expect("lines should be object");
    let prompts = json["prompts"]
        .as_object()
        .expect("prompts should be object");

    // Should have AI line mapped to prompt
    assert!(!lines.is_empty());
    assert!(!prompts.is_empty());
}

#[test]
fn test_blame_format_default_with_flags() {
    // Output format: Default format with various flags
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);
    repo.stage_all_and_commit("Test").unwrap();

    // Test with -e (show email)
    let output = repo.git_ai(&["blame", "-e", "test.txt"]).unwrap();
    assert!(output.contains("@"));

    // Test with -n (show line numbers)
    let output = repo.git_ai(&["blame", "-n", "test.txt"]).unwrap();
    assert!(output.contains(" 1 "));
    assert!(output.contains(" 2 "));

    // Test with -f (show filename)
    let output = repo.git_ai(&["blame", "-f", "test.txt"]).unwrap();
    assert!(output.contains("test.txt"));

    // Test with -s (suppress author)
    let output = repo.git_ai(&["blame", "-s", "test.txt"]).unwrap();
    assert!(!output.contains("Test User"));
}

// =============================================================================
// Ignore Whitespace Tests
// =============================================================================

#[test]
fn test_blame_ignore_whitespace() {
    // Ignore whitespace: -w flag should ignore whitespace changes
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line1"]);
    let commit1 = repo.stage_all_and_commit("Original").unwrap();

    file.set_contents(crate::lines!["  Line1"]); // Add leading spaces
    repo.stage_all_and_commit("Add spaces").unwrap();

    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");

    let options = GitAiBlameOptions {
        ignore_whitespace: true,
        ..Default::default()
    };

    let hunks = gitai_repo.blame_hunks("test.txt", 1, 1, &options).unwrap();

    // With ignore whitespace, should attribute to original commit
    assert!(hunks[0].commit_sha.starts_with(&commit1.commit_sha[..7]));
}

// =============================================================================
// Abbrev Tests - Hash abbreviation
// =============================================================================

#[test]
fn test_blame_abbrev_custom_length() {
    // Abbrev: Custom hash abbreviation length
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Test").unwrap();

    let output = repo
        .git_ai(&["blame", "--abbrev", "10", "test.txt"])
        .unwrap();

    // Boundary commits may be prefixed with '^' in default format.
    let first_field = output.split_whitespace().next().unwrap();
    let hash = first_field.trim_start_matches('^');
    assert!(
        (10..=40).contains(&hash.len()),
        "expected abbreviated hash length in [10,40], got {}",
        hash.len()
    );
}

#[test]
fn test_blame_long_rev() {
    // Long rev: -l flag shows full 40-character hash
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Test").unwrap();

    let output = repo.git_ai(&["blame", "-l", "test.txt"]).unwrap();

    // Boundary commits may be prefixed with '^' in default format.
    let first_field = output.split_whitespace().next().unwrap();
    let hash = first_field.trim_start_matches('^');
    assert_eq!(hash.len(), 40);
}

// =============================================================================
// Date Format Tests
// =============================================================================

#[test]
fn test_blame_date_format_short() {
    // Date format: --date short shows YYYY-MM-DD
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1"]);
    repo.stage_all_and_commit("Test").unwrap();

    let output = repo
        .git_ai(&["blame", "--date", "short", "test.txt"])
        .unwrap();

    // Should contain date in YYYY-MM-DD format
    assert!(output.contains("-")); // Date separator
    let parts: Vec<&str> = output.split_whitespace().collect();
    let date_field = parts
        .iter()
        .find(|s| s.len() == 10 && s.matches('-').count() == 2);
    assert!(date_field.is_some(), "Should have YYYY-MM-DD date");
}

crate::reuse_tests_in_worktree!(
    test_blame_format_porcelain_basic,
    test_blame_format_line_porcelain,
    test_blame_format_incremental,
    test_blame_format_json_structure,
    test_blame_format_default_with_flags,
    test_blame_ignore_whitespace,
    test_blame_abbrev_custom_length,
    test_blame_long_rev,
    test_blame_date_format_short,
);
