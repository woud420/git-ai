use super::{ExpectedLineExt, TestRepo};

// =============================================================================
// Edge Cases - Empty files, boundary commits, renamed files
// =============================================================================

#[test]
fn test_blame_edge_empty_file() {
    // Edge case: Blame on an empty file
    let repo = TestRepo::new();
    let file_path = repo.path().join("empty.txt");
    std::fs::write(&file_path, "").unwrap();

    repo.git(&["add", "empty.txt"]).unwrap();
    repo.stage_all_and_commit("Empty file").unwrap();

    // Empty files return an error because line range 1:0 is invalid
    let result = repo.git_ai(&["blame", "empty.txt"]);
    assert!(
        result.is_err(),
        "Empty file should fail with line range error"
    );
}

#[test]
fn test_blame_edge_single_line_file() {
    // Edge case: File with only one line
    let repo = TestRepo::new();
    let mut file = repo.filename("single.txt");

    file.set_contents(crate::lines!["Only line".ai()]);
    repo.stage_all_and_commit("Single line").unwrap();

    let output = repo.git_ai(&["blame", "single.txt"]).unwrap();

    assert!(output.contains("Only line"));
    assert_eq!(output.lines().count(), 1);
}

#[test]
fn test_blame_edge_large_file() {
    // Edge case: Large file with many lines
    let repo = TestRepo::new();
    let file = repo.filename("large.txt");

    let mut lines = Vec::new();
    for i in 1..=1000 {
        lines.push(format!("Line {}", i));
    }
    std::fs::write(file.file_path.clone(), lines.join("\n") + "\n").unwrap();

    repo.stage_all_and_commit("Large file").unwrap();

    let output = repo.git_ai(&["blame", "large.txt"]).unwrap();

    // Should contain all lines
    assert!(output.contains("Line 1"));
    assert!(output.contains("Line 500"));
    assert!(output.contains("Line 1000"));
    assert_eq!(output.lines().count(), 1000);
}

#[test]
fn test_blame_edge_file_with_unicode() {
    // Edge case: File with unicode content
    let repo = TestRepo::new();
    let mut file = repo.filename("unicode.txt");

    file.set_contents(crate::lines![
        "Hello 世界".ai(),
        "Emoji: 🚀 🎉".ai(),
        "Greek: αβγδ".human()
    ]);

    repo.stage_all_and_commit("Unicode content").unwrap();

    let output = repo.git_ai(&["blame", "unicode.txt"]).unwrap();

    assert!(output.contains("世界"));
    assert!(output.contains("🚀"));
    assert!(output.contains("αβγδ"));
}

#[test]
fn test_blame_edge_file_with_very_long_lines() {
    // Edge case: File with very long lines
    let repo = TestRepo::new();
    let mut file = repo.filename("longlines.txt");

    let long_line = "a".repeat(5000);
    file.set_contents(crate::lines![long_line.as_str().ai()]);

    repo.stage_all_and_commit("Long line").unwrap();

    let output = repo.git_ai(&["blame", "longlines.txt"]).unwrap();

    // Should handle long lines without error
    assert!(output.len() > 5000);
}

#[test]
fn test_blame_edge_boundary_commit_flag() {
    // Edge case: Boundary commit with -b flag
    let repo = TestRepo::new();
    repo.git(&["checkout", "--orphan", "boundary-test"])
        .unwrap();
    let mut file = repo.filename("boundary.txt");

    file.set_contents(crate::lines!["Initial line"]);
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "-m", "Initial commit"]).unwrap();

    let output = repo.git_ai(&["blame", "-b", "boundary.txt"]).unwrap();

    // With -b, boundary commits should show empty hash
    assert!(output.contains("        ") || output.contains("^"));
}

#[test]
fn test_blame_edge_renamed_file() {
    // Edge case: Blame on a renamed file
    let repo = TestRepo::new();
    let mut file = repo.filename("original.txt");

    file.set_contents(crate::lines!["Original content".ai()]);
    repo.stage_all_and_commit("Add original").unwrap();

    // Rename the file
    let old_path = repo.path().join("original.txt");
    let new_path = repo.path().join("renamed.txt");
    std::fs::rename(&old_path, &new_path).unwrap();

    repo.git(&["add", "original.txt", "renamed.txt"]).unwrap();
    repo.stage_all_and_commit("Rename file").unwrap();

    let output = repo.git_ai(&["blame", "renamed.txt"]).unwrap();

    assert!(output.contains("Original content"));
}

#[test]
fn test_blame_edge_whitespace_only_lines() {
    // Edge case: Lines containing only whitespace
    let repo = TestRepo::new();
    let file = repo.filename("whitespace.txt");

    std::fs::write(file.file_path.clone(), "Line 1\n   \n\t\t\nLine 4").unwrap();
    repo.git(&["add", "whitespace.txt"]).unwrap();
    repo.stage_all_and_commit("Whitespace lines").unwrap();

    let output = repo.git_ai(&["blame", "whitespace.txt"]).unwrap();

    // Should handle whitespace-only lines
    assert_eq!(output.lines().count(), 4);
}

// =============================================================================
// Stress Tests - Performance and robustness
// =============================================================================

#[test]
fn test_blame_stress_many_small_hunks() {
    // Stress: Many small hunks with alternating authorship
    let repo = TestRepo::new();
    let file = repo.filename("alternating.txt");

    let mut lines = Vec::new();
    for i in 0..100 {
        if i % 2 == 0 {
            lines.push(format!("Human {}", i));
        } else {
            lines.push(format!("AI {}", i));
        }
    }
    std::fs::write(file.file_path.clone(), lines.join("\n") + "\n").unwrap();

    repo.stage_all_and_commit("Alternating authorship").unwrap();

    let output = repo.git_ai(&["blame", "alternating.txt"]).unwrap();

    assert!(output.contains("Human 0"));
    assert!(output.contains("AI 99") || output.contains("Human 98"));
}

#[test]
fn test_blame_stress_deeply_nested_path() {
    // Stress: File in deeply nested directory structure
    let repo = TestRepo::new();

    let deep_path = repo
        .path()
        .join("a")
        .join("b")
        .join("c")
        .join("d")
        .join("e")
        .join("f")
        .join("g")
        .join("h");
    std::fs::create_dir_all(&deep_path).unwrap();

    let file_path = deep_path.join("deep.txt");
    std::fs::write(&file_path, "Deep content\n").unwrap();

    repo.git(&["add", "a/b/c/d/e/f/g/h/deep.txt"]).unwrap();
    repo.stage_all_and_commit("Deep file").unwrap();

    let output = repo.git_ai(&["blame", "a/b/c/d/e/f/g/h/deep.txt"]).unwrap();

    assert!(output.contains("Deep content"));
}

crate::reuse_tests_in_worktree!(
    test_blame_edge_empty_file,
    test_blame_edge_single_line_file,
    test_blame_edge_large_file,
    test_blame_edge_file_with_unicode,
    test_blame_edge_file_with_very_long_lines,
    test_blame_edge_boundary_commit_flag,
    test_blame_edge_renamed_file,
    test_blame_edge_whitespace_only_lines,
    test_blame_stress_many_small_hunks,
    test_blame_stress_deeply_nested_path,
);
