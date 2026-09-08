use super::*;

#[test]
fn test_capture_diff_slices_simple() {
    let old = vec!["a", "b", "c"];
    let new = vec!["a", "x", "c"];

    let ops = capture_diff_slices(&old, &new);

    assert_eq!(ops.len(), 3);
    assert!(matches!(
        ops[0],
        DiffOp::Equal {
            old_index: 0,
            new_index: 0,
            len: 1
        }
    ));
    assert!(matches!(
        ops[1],
        DiffOp::Replace {
            old_index: 1,
            old_len: 1,
            new_index: 1,
            new_len: 1
        }
    ));
    assert!(matches!(
        ops[2],
        DiffOp::Equal {
            old_index: 2,
            new_index: 2,
            len: 1
        }
    ));
}

#[test]
fn test_capture_diff_slices_insert() {
    let old = vec!["a", "c"];
    let new = vec!["a", "b", "c"];

    let ops = capture_diff_slices(&old, &new);

    assert_eq!(ops.len(), 3);
    assert!(matches!(
        ops[0],
        DiffOp::Equal {
            old_index: 0,
            new_index: 0,
            len: 1
        }
    ));
    assert!(matches!(
        ops[1],
        DiffOp::Insert {
            old_index: 1,
            new_index: 1,
            new_len: 1
        }
    ));
    assert!(matches!(
        ops[2],
        DiffOp::Equal {
            old_index: 1,
            new_index: 2,
            len: 1
        }
    ));
}

#[test]
fn test_capture_diff_slices_delete() {
    let old = vec!["a", "b", "c"];
    let new = vec!["a", "c"];

    let ops = capture_diff_slices(&old, &new);

    assert_eq!(ops.len(), 3);
    assert!(matches!(
        ops[0],
        DiffOp::Equal {
            old_index: 0,
            new_index: 0,
            len: 1
        }
    ));
    assert!(matches!(
        ops[1],
        DiffOp::Delete {
            old_index: 1,
            old_len: 1,
            new_index: 1
        }
    ));
    assert!(matches!(
        ops[2],
        DiffOp::Equal {
            old_index: 2,
            new_index: 1,
            len: 1
        }
    ));
}

#[test]
fn test_compute_line_changes() {
    let old = "line1\nline2\nline3\n";
    let new = "line1\nmodified\nline3\n";

    let changes = compute_line_changes(old, new);

    let tags: Vec<_> = changes.iter().map(|c| c.tag().clone()).collect();
    assert_eq!(
        tags,
        vec![
            LineChangeTag::Equal,
            LineChangeTag::Delete,
            LineChangeTag::Insert,
            LineChangeTag::Equal,
        ]
    );
}

#[test]
fn test_compute_line_changes_insert_only() {
    let old = "line1\nline2\n";
    let new = "line1\nline2\nline3\n";

    let changes = compute_line_changes(old, new);

    let tags: Vec<_> = changes.iter().map(|c| c.tag().clone()).collect();
    assert_eq!(
        tags,
        vec![
            LineChangeTag::Equal,
            LineChangeTag::Equal,
            LineChangeTag::Insert,
        ]
    );
}

#[test]
fn test_split_lines_with_terminators() {
    let s = "line1\nline2\nline3";
    let lines = split_lines_with_terminators(s);
    assert_eq!(lines, vec!["line1\n", "line2\n", "line3"]);

    let s_trailing = "line1\nline2\n";
    let lines_trailing = split_lines_with_terminators(s_trailing);
    assert_eq!(lines_trailing, vec!["line1\n", "line2\n"]);
}

// ====================================================================
// CRLF / LF normalization tests
// ====================================================================

#[test]
fn test_compute_line_changes_crlf_to_lf_identical_content() {
    // Old file has CRLF, new file has LF. Content is identical otherwise.
    // Should produce NO changes (all Equal).
    let old = "line1\r\nline2\r\nline3\r\n";
    let new = "line1\nline2\nline3\n";

    let changes = compute_line_changes(old, new);

    let tags: Vec<_> = changes.iter().map(|c| c.tag().clone()).collect();
    assert_eq!(
        tags,
        vec![
            LineChangeTag::Equal,
            LineChangeTag::Equal,
            LineChangeTag::Equal,
        ],
        "CRLF→LF conversion with identical content should produce no changes"
    );
}

#[test]
fn test_compute_line_changes_lf_to_crlf_identical_content() {
    // Old file has LF, new file has CRLF. Content is identical otherwise.
    // Should produce NO changes (all Equal).
    let old = "line1\nline2\nline3\n";
    let new = "line1\r\nline2\r\nline3\r\n";

    let changes = compute_line_changes(old, new);

    let tags: Vec<_> = changes.iter().map(|c| c.tag().clone()).collect();
    assert_eq!(
        tags,
        vec![
            LineChangeTag::Equal,
            LineChangeTag::Equal,
            LineChangeTag::Equal,
        ],
        "LF→CRLF conversion with identical content should produce no changes"
    );
}

#[test]
fn test_compute_line_changes_crlf_old_with_real_addition() {
    // Old file has CRLF (100-line-like scenario), new file has LF with real additions.
    // Only the actual new lines should show as Insert.
    let old = "line1\r\nline2\r\nline3\r\n";
    let new = "line1\nline2\nnew_line\nline3\n";

    let changes = compute_line_changes(old, new);

    let tags: Vec<_> = changes.iter().map(|c| c.tag().clone()).collect();
    assert_eq!(
        tags,
        vec![
            LineChangeTag::Equal,
            LineChangeTag::Equal,
            LineChangeTag::Insert,
            LineChangeTag::Equal,
        ],
        "Only the genuinely new line should be an Insert, not CRLF→LF conversions"
    );
}

#[test]
fn test_compute_line_changes_mixed_crlf_with_modification() {
    // Old has CRLF, new has LF. One line is actually modified.
    let old = "line1\r\nline2\r\nline3\r\n";
    let new = "line1\nmodified\nline3\n";

    let changes = compute_line_changes(old, new);

    let tags: Vec<_> = changes.iter().map(|c| c.tag().clone()).collect();
    assert_eq!(
        tags,
        vec![
            LineChangeTag::Equal,
            LineChangeTag::Delete,
            LineChangeTag::Insert,
            LineChangeTag::Equal,
        ],
        "Only the actually-modified line should show as Delete+Insert"
    );
}

#[test]
fn test_normalize_line_endings_does_not_allocate_for_bare_carriage_returns() {
    let normalized = normalize_line_endings("one\rtwo");

    assert!(matches!(normalized, std::borrow::Cow::Borrowed(_)));
}

#[test]
fn test_content_eq_ignoring_line_endings() {
    assert!(content_eq_ignoring_line_endings(
        "one\r\ntwo\nthree\r\n",
        "one\ntwo\r\nthree\n"
    ));
    assert!(!content_eq_ignoring_line_endings("one\rtwo", "one\ntwo"));
    assert!(!content_eq_ignoring_line_endings("one\n", "two\n"));
}
