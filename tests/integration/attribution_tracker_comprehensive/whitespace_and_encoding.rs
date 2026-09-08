use super::{Attribution, AttributionTracker};

// =============================================================================
// Whitespace Handling Tests
// =============================================================================

#[test]
fn test_tracker_whitespace_only_addition() {
    // Test that whitespace-only additions are handled correctly
    let tracker = AttributionTracker::new();
    let old_content = "line 1\nline 2\n";
    let new_content = "line 1\n\n\nline 2\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "ai-2".to_string(), 2000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 3000)
        .unwrap();

    // Original attributions should be preserved, potentially with whitespace attributed
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-1"));
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-2"));
}

#[test]
fn test_tracker_whitespace_only_deletion() {
    // Test that whitespace-only deletions are handled correctly
    let tracker = AttributionTracker::new();
    let old_content = "line 1\n\n\nline 2\n";
    let new_content = "line 1\nline 2\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 9, "ai-2".to_string(), 2000),
        Attribution::new(9, 16, "ai-3".to_string(), 3000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 4000)
        .unwrap();

    // Should preserve non-whitespace attributions
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-1"));
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-3"));
}

#[test]
fn test_tracker_trailing_whitespace_changes() {
    // Test trailing whitespace changes
    let tracker = AttributionTracker::new();
    let old_content = "line 1\nline 2\n";
    let new_content = "line 1  \nline 2  \n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "ai-2".to_string(), 2000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 3000)
        .unwrap();

    // Original attributions should be preserved
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-1"));
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-2"));
}

#[test]
fn test_tracker_indentation_changes() {
    // Test indentation changes
    let tracker = AttributionTracker::new();
    let old_content = "line 1\nline 2\n";
    let new_content = "    line 1\n    line 2\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "ai-2".to_string(), 2000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 3000)
        .unwrap();

    // Should have attributions for both original content and added indentation
    assert!(!new_attrs.is_empty());
}

// =============================================================================
// Unicode and Special Character Tests
// =============================================================================

#[test]
fn test_tracker_unicode_content() {
    // Test handling of Unicode characters
    let tracker = AttributionTracker::new();
    let old_content = "Hello 世界\n";
    let new_content = "Hello 世界！\n";

    let old_attrs = vec![Attribution::new(0, 13, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 2000)
        .unwrap();

    // Should handle Unicode properly
    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_emoji_content() {
    // Test handling of emoji characters
    let tracker = AttributionTracker::new();
    let old_content = "Hello 👋\n";
    let new_content = "Hello 👋🌍\n";

    let old_attrs = vec![Attribution::new(0, 11, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_mixed_unicode_content() {
    // Test mixed ASCII and Unicode content
    let tracker = AttributionTracker::new();
    let old_content = "ASCII текст 中文 🎉\n";
    let new_content = "ASCII текст 中文 🎉 more\n";

    let old_attrs = vec![Attribution::new(0, 28, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 2000)
        .unwrap();

    assert!(new_attrs.iter().any(|a| a.author_id == "ai-1"));
    assert!(new_attrs.iter().any(|a| a.author_id == "current-author"));
}

#[test]
fn test_tracker_zero_width_unicode() {
    // Test zero-width Unicode characters
    let tracker = AttributionTracker::new();
    let old_content = "test\u{200B}content\n"; // Zero-width space
    let new_content = "test\u{200B}content\u{200B}\n";

    let old_attrs = vec![Attribution::new(0, 16, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_special_characters() {
    // Test special characters and escape sequences
    let tracker = AttributionTracker::new();
    let old_content = "line\\twith\\ttabs\n";
    let new_content = "line\\twith\\ttabs\\n\n";

    let old_attrs = vec![Attribution::new(0, 16, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_no_newline_at_end() {
    // Test content without trailing newline
    let tracker = AttributionTracker::new();
    let old_content = "no newline";
    let new_content = "no newline modified";

    let old_attrs = vec![Attribution::new(0, 10, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_only_newlines() {
    // Test content that's only newlines
    let tracker = AttributionTracker::new();
    let old_content = "\n\n\n";
    let new_content = "\n\n\n\n";

    let old_attrs = vec![Attribution::new(0, 3, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_windows_line_endings() {
    // Test Windows line endings (CRLF)
    let tracker = AttributionTracker::new();
    let old_content = "line 1\r\nline 2\r\n";
    let new_content = "line 1\r\nmodified\r\n";

    let old_attrs = vec![
        Attribution::new(0, 8, "ai-1".to_string(), 1000),
        Attribution::new(8, 16, "ai-2".to_string(), 2000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 3000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_mixed_line_endings() {
    // Test mixed line endings
    let tracker = AttributionTracker::new();
    let old_content = "line 1\nline 2\r\nline 3\n";
    let new_content = "line 1\nmodified\r\nline 3\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 15, "ai-2".to_string(), 2000),
        Attribution::new(15, 22, "ai-3".to_string(), 3000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 4000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_binary_like_content() {
    // Test handling content that looks binary-ish but is still text
    let tracker = AttributionTracker::new();
    let old_content = "\x00\x01\x02\x03\n";
    let new_content = "\x00\x01\x7F\x02\x03\n";

    let old_attrs = vec![Attribution::new(0, 5, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_all_whitespace_file() {
    // Test a file that's entirely whitespace
    let tracker = AttributionTracker::new();
    let old_content = "   \n\t\t\n  \n";
    let new_content = "   \n\t\t\t\n  \n";

    let old_attrs = vec![Attribution::new(0, 10, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

crate::reuse_tests_in_worktree!(
    test_tracker_whitespace_only_addition,
    test_tracker_whitespace_only_deletion,
    test_tracker_trailing_whitespace_changes,
    test_tracker_indentation_changes,
    test_tracker_unicode_content,
    test_tracker_emoji_content,
    test_tracker_mixed_unicode_content,
    test_tracker_zero_width_unicode,
    test_tracker_special_characters,
    test_tracker_no_newline_at_end,
    test_tracker_only_newlines,
    test_tracker_windows_line_endings,
    test_tracker_mixed_line_endings,
    test_tracker_binary_like_content,
    test_tracker_all_whitespace_file,
);
