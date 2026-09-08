use super::{ExpectedLineExt, TestRepo, Value};

// ============================================================================
// JSON Output Tests (complementing existing tests)
// ============================================================================

#[test]
fn test_diff_json_structure() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("json_struct.rs");
    file.set_contents(crate::lines!["fn old() {}".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Make AI changes
    file.set_contents(crate::lines!["fn new() {}".ai()]);
    let commit = repo.stage_all_and_commit("AI changes").unwrap();

    // Run diff with --json
    let output = repo
        .git_ai(&["diff", &commit.commit_sha, "--json"])
        .expect("diff --json should succeed");

    // Parse JSON
    let json: Value = serde_json::from_str(&output).expect("Should be valid JSON");

    // Verify top-level structure
    assert!(
        json.get("files").is_some(),
        "JSON should have 'files' field"
    );
    assert!(
        json.get("prompts").is_some(),
        "JSON should have 'prompts' field"
    );
    assert!(
        json.get("hunks").is_some(),
        "JSON should have 'hunks' field"
    );
    assert!(
        json.get("commits").is_some(),
        "JSON should have 'commits' field"
    );

    // Verify files is an object
    assert!(json["files"].is_object(), "files should be an object (map)");

    // Verify prompts is an object
    assert!(
        json["prompts"].is_object(),
        "prompts should be an object (map)"
    );
    assert!(json["hunks"].is_array(), "hunks should be an array");
    assert!(
        json["commits"].is_object(),
        "commits should be an object (map)"
    );
}

#[test]
fn test_diff_json_file_structure() {
    let repo = TestRepo::new();

    // Create commit with AI changes
    let mut file = repo.filename("file_struct.ts");
    file.set_contents(crate::lines!["const x = 1;".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    file.set_contents(crate::lines!["const x = 2;".ai()]);
    let commit = repo.stage_all_and_commit("Update x").unwrap();

    // Run diff with --json
    let output = repo
        .git_ai(&["diff", &commit.commit_sha, "--json"])
        .expect("diff --json should succeed");

    // Parse JSON
    let json: Value = serde_json::from_str(&output).expect("Should be valid JSON");

    // Get the file entry
    let files = json["files"].as_object().expect("files should be object");
    assert!(!files.is_empty(), "Should have at least one file");

    let file_entry = files.values().next().expect("Should have a file");

    // Verify file structure
    assert!(
        file_entry.get("annotations").is_some(),
        "File should have annotations"
    );
    assert!(file_entry.get("diff").is_some(), "File should have diff");
    assert!(
        file_entry.get("base_content").is_some(),
        "File should have base_content"
    );
}

#[test]
fn test_diff_json_annotations_format() {
    let repo = TestRepo::new();

    // Create commit with AI changes
    let mut file = repo.filename("annotations.rs");
    file.set_contents(crate::lines!["Line 1".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".ai()
    ]);
    let commit = repo.stage_all_and_commit("Add AI lines").unwrap();

    // Run diff with --json
    let output = repo
        .git_ai(&["diff", &commit.commit_sha, "--json"])
        .expect("diff --json should succeed");

    // Parse JSON
    let json: Value = serde_json::from_str(&output).expect("Should be valid JSON");

    // Verify annotations structure
    let files = json["files"].as_object().expect("files should be object");
    if let Some(file_entry) = files.values().next() {
        let annotations = &file_entry["annotations"];
        assert!(
            annotations.is_object(),
            "annotations should be an object (map)"
        );
    }
}

#[test]
fn test_diff_json_base_content_accuracy() {
    let repo = TestRepo::new();

    // Create file with specific content
    let initial_content = "const x = 1;\nconst y = 2;\n";
    let file_path = repo.path().join("base_test.js");
    std::fs::write(&file_path, initial_content).unwrap();
    repo.stage_all_and_commit("Initial").unwrap();

    // Modify content
    std::fs::write(&file_path, "const x = 1;\nconst z = 3;\n").unwrap();
    let commit = repo.stage_all_and_commit("Modify").unwrap();

    // Run diff with --json
    let output = repo
        .git_ai(&["diff", &commit.commit_sha, "--json"])
        .expect("diff --json should succeed");

    // Parse JSON
    let json: Value = serde_json::from_str(&output).expect("Should be valid JSON");

    // Verify base_content matches original
    let files = json["files"].as_object().expect("files should be object");
    let file_entry = &files["base_test.js"];
    let base_content = file_entry["base_content"]
        .as_str()
        .expect("base_content should be string");

    assert_eq!(
        base_content, initial_content,
        "base_content should match original file"
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_diff_invalid_commit_ref() {
    let repo = TestRepo::new();

    // Create a commit so repo is not empty
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Content".human()]);
    repo.stage_all_and_commit("Test").unwrap();

    // Try to diff non-existent commit
    let result = repo.git_ai(&["diff", "nonexistent123"]);

    // Should fail gracefully
    assert!(result.is_err(), "diff with invalid ref should fail");
}

// ============================================================================
// Special Content Tests
// ============================================================================

#[test]
fn test_diff_with_very_long_lines() {
    let repo = TestRepo::new();

    // Create file with very long line
    let long_line = "x".repeat(1000);
    let mut file = repo.filename("long.txt");
    file.set_contents(vec![long_line.clone().human()]);
    repo.stage_all_and_commit("Long line").unwrap();

    // Modify the long line
    let modified = format!("{}y", long_line);
    file.set_contents(vec![modified.ai()]);
    let commit = repo.stage_all_and_commit("Modify long line").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff with long lines should succeed");

    // Should handle long lines
    assert!(
        output.contains("+") && output.contains("-"),
        "Should show diff"
    );
}

#[test]
fn test_diff_with_special_regex_chars() {
    let repo = TestRepo::new();

    // Create file with special characters that might affect regex
    let mut file = repo.filename("special.txt");
    file.set_contents(crate::lines![
        "Line with $pecial [chars] (and) {braces}".human()
    ]);
    repo.stage_all_and_commit("Special chars").unwrap();

    // Modify
    file.set_contents(crate::lines![
        "Line with $pecial [chars] (and) {braces} modified".ai()
    ]);
    let commit = repo.stage_all_and_commit("Modify special").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff with special chars should succeed");

    // Should handle special characters
    assert!(
        output.contains("$pecial") || output.contains("chars"),
        "Should show content with special chars"
    );
}

#[test]
fn test_diff_whitespace_only_changes() {
    let repo = TestRepo::new();

    // Create file
    let mut file = repo.filename("whitespace.rs");
    file.set_contents(crate::lines!["fn test() {".human(), "}".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Change whitespace only
    file.set_contents(crate::lines![
        "fn test() {".human(),
        "    ".human(),
        "}".human()
    ]);
    let commit = repo.stage_all_and_commit("Add whitespace").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff with whitespace changes should succeed");

    // Should show the whitespace change
    assert!(
        output.contains("+") || output.contains("-"),
        "Should show whitespace changes"
    );
}

// ============================================================================
// Compatibility Tests
// ============================================================================

#[test]
fn test_diff_works_with_submodules() {
    let repo = TestRepo::new();

    // Create a simple file (submodule handling is complex, just test basic compatibility)
    let mut file = repo.filename("main.rs");
    file.set_contents(crate::lines!["fn main() {}".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    file.set_contents(crate::lines!["fn main() {}".human(), "fn helper() {}".ai()]);
    let commit = repo.stage_all_and_commit("Add helper").unwrap();

    // Run diff (should work even if repo could theoretically have submodules)
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff should work");

    assert!(output.contains("helper"), "Should show the change");
}

#[test]
fn test_diff_attribution_consistency() {
    let repo = TestRepo::new();

    // Create commit with AI changes
    let mut file = repo.filename("consistency.rs");
    file.set_contents(crate::lines!["Line 1".ai(), "Line 2".ai()]);
    let commit = repo.stage_all_and_commit("AI commit").unwrap();

    // Run diff multiple times
    let output1 = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff 1 should succeed");
    let output2 = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff 2 should succeed");

    // Results should be identical (deterministic)
    assert_eq!(
        output1, output2,
        "Multiple diff runs should produce identical output"
    );
}

crate::reuse_tests_in_worktree!(
    test_diff_json_structure,
    test_diff_json_file_structure,
    test_diff_json_annotations_format,
    test_diff_json_base_content_accuracy,
    test_diff_invalid_commit_ref,
    test_diff_with_very_long_lines,
    test_diff_with_special_regex_chars,
    test_diff_whitespace_only_changes,
    test_diff_works_with_submodules,
    test_diff_attribution_consistency,
);
