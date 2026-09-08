use super::{ExpectedLineExt, TestRepo, assert_diff_lines_exact, parse_diff_output};

#[test]
fn test_diff_single_commit() {
    let repo = TestRepo::new();

    // Initial commit
    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Line 1".human(), "Line 2".human()]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Second commit with AI and human changes
    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2 modified".ai(),
        "Line 3 new".ai(),
        "Line 4 human".human()
    ]);
    let second = repo.stage_all_and_commit("Mixed changes").unwrap();

    // Run git-ai diff on the second commit
    let output = repo
        .git_ai(&["diff", &second.commit_sha])
        .expect("git-ai diff should succeed");

    // Parse diff output
    let lines = parse_diff_output(&output);

    // Verify exact lines
    // Should have: -Line 2, +Line 2 modified, +Line 3 new, +Line 4 human
    assert!(
        lines.len() >= 4,
        "Should have at least 4 diff lines, got {}: {:?}",
        lines.len(),
        lines
    );

    // Find the deletion of Line 2
    let line2_deletion = lines
        .iter()
        .find(|l| l.prefix == "-" && l.content.contains("Line 2"));
    assert!(line2_deletion.is_some(), "Should have deletion of Line 2");

    // Find additions
    let line2_addition = lines
        .iter()
        .find(|l| l.prefix == "+" && l.content.contains("Line 2 modified"));
    assert!(
        line2_addition.is_some(),
        "Should have addition of 'Line 2 modified'"
    );
    if let Some(line) = line2_addition {
        assert!(
            line.attribution
                .as_ref()
                .map(|a| a.contains("ai"))
                .unwrap_or(false),
            "Line 2 modified should have AI attribution, got: {:?}",
            line.attribution
        );
    }

    let line3_addition = lines
        .iter()
        .find(|l| l.prefix == "+" && l.content.contains("Line 3 new"));
    assert!(
        line3_addition.is_some(),
        "Should have addition of 'Line 3 new'"
    );
    if let Some(line) = line3_addition {
        assert!(
            line.attribution
                .as_ref()
                .map(|a| a.contains("ai"))
                .unwrap_or(false),
            "Line 3 new should have AI attribution, got: {:?}",
            line.attribution
        );
    }

    let line4_addition = lines
        .iter()
        .find(|l| l.prefix == "+" && l.content.contains("Line 4 human"));
    assert!(
        line4_addition.is_some(),
        "Should have addition of 'Line 4 human'"
    );
}

#[test]
fn test_diff_output_format() {
    let repo = TestRepo::new();

    // Create a simple diff
    let mut file = repo.filename("format.txt");
    file.set_contents(crate::lines!["old".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    file.set_contents(crate::lines!["new".ai()]);
    let commit = repo.stage_all_and_commit("Change").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff should succeed");

    // Verify standard git diff format elements
    assert!(output.contains("diff --git"), "Should have diff header");
    assert!(output.contains("---"), "Should have old file marker");
    assert!(output.contains("+++"), "Should have new file marker");
    assert!(output.contains("@@"), "Should have hunk header");

    // Parse and verify exact sequence of diff lines
    let lines = parse_diff_output(&output);

    assert_diff_lines_exact(
        &lines,
        &[
            ("-", "old", None),       // Deletion (may have no-data or human)
            ("+", "new", Some("ai")), // Addition with AI attribution
        ],
    );
}

#[test]
fn test_diff_error_on_no_args() {
    let repo = TestRepo::new();

    // Try to run diff without arguments
    let result = repo.git_ai(&["diff"]);

    // Should fail with error
    assert!(result.is_err(), "git-ai diff without arguments should fail");
}

#[test]
fn test_diff_json_output_with_escaped_newlines() {
    let repo = TestRepo::new();

    // Initial commit with text.split("\n")
    let mut file = repo.filename("utils.ts");
    file.set_contents(crate::lines![r#"const lines = text.split("\n")"#.human()]);
    repo.stage_all_and_commit("Initial split implementation")
        .unwrap();

    // Modify to other_text.split("\n\n")
    file.set_contents(crate::lines![
        r#"const lines = other_text.split("\n\n")"#.ai()
    ]);
    let commit = repo
        .stage_all_and_commit("Update split to use double newline")
        .unwrap();

    // Run git-ai diff with --json flag
    let output = repo
        .git_ai(&["diff", &commit.commit_sha, "--json"])
        .expect("git-ai diff --json should succeed");

    // Parse JSON to verify it's valid
    let json: serde_json::Value =
        serde_json::from_str(&output).expect("Output should be valid JSON");

    // Verify newlines are properly escaped in the base_content
    let files = json.get("files").unwrap().as_object().unwrap();
    let utils_file = files.get("utils.ts").unwrap();
    let base_content = utils_file.get("base_content").unwrap().as_str().unwrap();
    assert!(
        base_content.contains(r#"text.split("\n")"#),
        "Base content should contain properly escaped newlines: text.split(\"\\n\"), got: {}",
        base_content
    );

    // Verify newlines are properly escaped in the diff content
    let diff = utils_file.get("diff").unwrap().as_str().unwrap();
    assert!(
        diff.contains(r#"text.split("\n")"#),
        "Diff should contain properly escaped newlines in old line: text.split(\"\\n\")"
    );
    assert!(
        diff.contains(r#"other_text.split("\n\n")"#),
        "Diff should contain properly escaped newlines in new line: other_text.split(\"\\n\\n\")"
    );

    // Print the JSON output for inspection
    println!("JSON output:\n{}", serde_json::to_string(&json).unwrap());
}

#[test]
fn test_diff_preserves_context_lines() {
    let repo = TestRepo::new();

    // Create file with multiple lines
    let mut file = repo.filename("context.txt");
    file.set_contents(crate::lines![
        "Context 1".human(),
        "Context 2".human(),
        "Context 3".human(),
        "Old line".human(),
        "Context 4".human(),
        "Context 5".human(),
        "Context 6".human()
    ]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Change one line in the middle
    file.set_contents(crate::lines![
        "Context 1".human(),
        "Context 2".human(),
        "Context 3".human(),
        "New line".ai(),
        "Context 4".human(),
        "Context 5".human(),
        "Context 6".human()
    ]);
    let commit = repo.stage_all_and_commit("Change middle").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff should succeed");

    // Should show context lines (lines starting with space)
    let context_count = output
        .lines()
        .filter(|l| l.starts_with(' ') && !l.starts_with("  "))
        .count();
    assert!(
        context_count >= 3,
        "Should show at least 3 context lines (default -U3)"
    );
}

#[test]
fn test_diff_exact_sequence_verification() {
    let repo = TestRepo::new();

    // Initial commit with 2 lines
    let mut file = repo.filename("sequence.rs");
    file.set_contents(crate::lines![
        "fn first() {}".human(),
        "fn second() {}".ai()
    ]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Modify: delete first, modify second, add third
    file.set_contents(crate::lines![
        "fn second_modified() {}".ai(),
        "fn third() {}".ai()
    ]);
    let commit = repo.stage_all_and_commit("Complex changes").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff should succeed");

    // Parse and verify EXACT order of every line
    let lines = parse_diff_output(&output);

    // Verify exact sequence with specific order and attribution
    // Git will show: delete both old lines, add both new lines
    assert_diff_lines_exact(
        &lines,
        &[
            ("-", "fn first()", None),                 // Delete human line
            ("-", "fn second()", None), // Delete AI line (no attribution on deletions)
            ("+", "fn second_modified()", Some("ai")), // Add AI line
            ("+", "fn third()", Some("ai")), // Add AI line
        ],
    );
}

crate::reuse_tests_in_worktree!(
    test_diff_single_commit,
    test_diff_output_format,
    test_diff_error_on_no_args,
    test_diff_json_output_with_escaped_newlines,
    test_diff_preserves_context_lines,
    test_diff_exact_sequence_verification,
);
