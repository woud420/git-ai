use super::TestFile;

#[test]
fn blame_parser_preserves_multi_word_author_and_email() {
    let line = "abc123 (Jane Mary Doe <jane@example.com> 2026-07-22 12:00:00 -0400 1) content";

    assert_eq!(
        TestFile::parse_blame_line_static(line),
        (
            "Jane Mary Doe <jane@example.com>".to_string(),
            "content".to_string(),
        )
    );
}

#[test]
fn ai_detection_ignores_agent_names_inside_email_addresses() {
    assert!(!TestFile::is_ai_author_helper(
        "Human Developer <amp@example.com>"
    ));
    assert!(TestFile::is_ai_author_helper(
        "GitHub Copilot <human@example.com>"
    ));
}

#[test]
fn malformed_blame_line_is_returned_as_unknown() {
    let line = "abc123 (Jane Doe 2026-07-22 12:00:00 -0400 1 content";

    assert_eq!(
        TestFile::parse_blame_line_static(line),
        ("unknown".to_string(), line.to_string())
    );
}

#[test]
fn committed_line_filter_only_removes_not_committed_yet() {
    let blame_output = "\
abc123 (Jane Doe 2026-07-22 12:00:00 -0400 1) committed human
000000 (Not Committed Yet 2026-07-22 12:00:00 -0400 2) pending
def456 (mock_ai 2026-07-22 12:00:00 -0400 3) committed ai
";

    let all_lines = TestFile::parse_blame_lines(blame_output);
    let committed_lines = TestFile::parse_committed_blame_lines(blame_output);

    assert_eq!(all_lines.len(), 3);
    assert_eq!(all_lines[1].0, "Not Committed Yet");
    assert_eq!(
        committed_lines,
        vec![
            ("Jane Doe".to_string(), "committed human".to_string()),
            ("mock_ai".to_string(), "committed ai".to_string()),
        ]
    );
}
