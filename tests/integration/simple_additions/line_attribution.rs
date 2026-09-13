use super::{ExpectedLineExt, TestRepo, fs};

#[test]
fn test_simple_additions_empty_repo() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line1", "Line 2".ai(), "Line 3".ai(),]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    file.assert_lines_and_blame(crate::lines!["Line1".human(), "Line 2".ai(), "Line 3".ai(),]);
}

#[test]
fn test_simple_additions_with_base_commit() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Base line 1", "Base line 2"]);

    repo.stage_all_and_commit("Base commit").unwrap();

    file.insert_at(
        2,
        crate::lines!["NEW LINEs From Claude!".ai(), "Hello".ai(), "World".ai(),],
    );

    repo.stage_all_and_commit("AI additions").unwrap();

    file.assert_lines_and_blame(crate::lines![
        "Base line 1".human(),
        "Base line 2".ai(),
        "NEW LINEs From Claude!".ai(),
        "Hello".ai(),
        "World".ai(),
    ]);
}

#[test]
fn test_simple_additions_on_top_of_ai_contributions() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2", "Line 3"]);

    repo.stage_all_and_commit("Base commit").unwrap();

    file.insert_at(3, crate::lines!["AI Line 1".ai(), "AI Line 2".ai(),]);

    repo.stage_all_and_commit("AI commit").unwrap();

    file.replace_at(3, "HUMAN EDITED AI LINE".human());

    repo.stage_all_and_commit("Human edits AI").unwrap();

    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "Line 2".human(),
        "Line 3".ai(),
        "HUMAN EDITED AI LINE".human(),
        "AI Line 2".ai(),
    ]);
}

#[test]
fn test_simple_additions_new_file_not_git_added() {
    let repo = TestRepo::new();
    let mut file = repo.filename("new_file.txt");

    // Create a new file with human lines, then add AI lines before any git add
    file.set_contents(crate::lines![
        "Line 1 from human",
        "Line 2 from human",
        "Line 3 from human",
        "Line 4 from AI".ai(),
        "Line 5 from AI".ai(),
    ]);

    let commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // All lines should be attributed correctly
    assert!(!commit.authorship_log.attestations.is_empty());

    file.assert_lines_and_blame(crate::lines![
        "Line 1 from human",
        "Line 2 from human",
        "Line 3 from human",
        "Line 4 from AI".ai(),
        "Line 5 from AI".ai(),
    ]);
}

#[test]
fn test_ai_human_interleaved_line_attribution() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Base line"]);

    repo.stage_all_and_commit("Base commit").unwrap();

    file.insert_at(
        1,
        crate::lines!["AI Line 1".ai(), "Human Line 1".human(), "AI Line 2".ai()],
    );

    repo.stage_all_and_commit("Interleaved commit").unwrap();

    file.assert_lines_and_blame(crate::lines![
        "Base line".ai(),
        "AI Line 1".ai(),
        "Human Line 1".ai(),
        "AI Line 2".ai(),
    ]);
}

#[test]
fn test_ai_adds_lines_multiple_commits() {
    // Test AI adding lines across multiple commits
    let repo = TestRepo::new();
    let mut file = repo.filename("test.ts");

    file.set_contents(crate::lines!["base_line", ""]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    file.insert_at(
        1,
        crate::lines!["ai_line1".ai(), "ai_line2".ai(), "ai_line3".ai(),],
    );

    repo.stage_all_and_commit("AI adds first batch").unwrap();

    file.insert_at(4, crate::lines!["ai_line4".ai(), "ai_line5".ai(),]);

    repo.stage_all_and_commit("AI adds second batch").unwrap();

    file.assert_lines_and_blame(crate::lines![
        "base_line".human(),
        "ai_line1".ai(),
        "ai_line2".ai(),
        "ai_line3".ai(),
        "ai_line4".ai(),
        "ai_line5".ai(),
    ]);
}

#[test]
fn test_with_duplicate_lines() {
    // This test verifies that squash merge correctly preserves AI authorship for duplicate lines
    let repo = TestRepo::new();
    let mut file = repo.filename("helpers.rs");

    // Create master branch with first function (human-authored)
    file.set_contents(crate::lines![
        "pub fn format_string(s: &str) -> String {",
        "    s.to_uppercase()",
        "}",
    ]);
    repo.stage_all_and_commit("Add format_string function")
        .unwrap();

    file = repo.filename("helpers.rs");
    file.assert_lines_and_blame(crate::lines![
        "pub fn format_string(s: &str) -> String {".human(),
        "    s.to_uppercase()".human(),
        "}".human(),
    ]);

    // AI adds a second function
    // The key test: the second `}` on line 6 is AI-authored, but there's already a `}` on line 3
    let file_path = repo.path().join("helpers.rs");
    fs::write(
        &file_path,
        "pub fn format_string(s: &str) -> String {\n    s.to_uppercase()\n}\npub fn reverse_string(s: &str) -> String {\n    s.chars().rev().collect()\n}",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    repo.stage_all_and_commit("AI adds reverse_string function")
        .unwrap();

    file = repo.filename("helpers.rs");
    file.assert_lines_and_blame(crate::lines![
        "pub fn format_string(s: &str) -> String {".human(),
        "    s.to_uppercase()".human(),
        "}".ai(), // This is the attribution for the AI closing brace. Not natural, but this is how git works!
        "pub fn reverse_string(s: &str) -> String {".ai(),
        "    s.chars().rev().collect()".ai(),
        "}".human(), // Is human, because of how git diffs work!
    ]);
}

crate::reuse_tests_in_worktree!(
    test_simple_additions_empty_repo,
    test_simple_additions_with_base_commit,
    test_simple_additions_on_top_of_ai_contributions,
    test_simple_additions_new_file_not_git_added,
    test_ai_human_interleaved_line_attribution,
);
