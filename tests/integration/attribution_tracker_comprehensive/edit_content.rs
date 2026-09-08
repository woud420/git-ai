use super::{Attribution, AttributionConfig, AttributionTracker};

// =============================================================================
// AttributionTracker Tests - Core update_attributions functionality
// =============================================================================

#[test]
fn test_tracker_no_changes_preserves_attributions() {
    // Test that identical content preserves all attributions
    let tracker = AttributionTracker::new();
    let content = "line 1\nline 2\nline 3\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "human-1".to_string(), 2000),
        Attribution::new(14, 21, "ai-2".to_string(), 3000),
    ];

    let new_attrs = tracker
        .update_attributions(content, content, &old_attrs, "current-author", 4000)
        .unwrap();

    assert_eq!(new_attrs.len(), 3);
    assert_eq!(new_attrs[0].author_id, "ai-1");
    assert_eq!(new_attrs[1].author_id, "human-1");
    assert_eq!(new_attrs[2].author_id, "ai-2");
}

#[test]
fn test_tracker_simple_addition_at_end() {
    // Test adding new content at the end
    let tracker = AttributionTracker::new();
    let old_content = "line 1\n";
    let new_content = "line 1\nline 2\n";

    let old_attrs = vec![Attribution::new(0, 7, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 2000)
        .unwrap();

    // Should preserve old attribution and add new one for added content
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-1"));
    assert!(new_attrs.iter().any(|a| a.author_id == "current-author"));
}

#[test]
fn test_tracker_simple_addition_at_start() {
    // Test adding new content at the start
    let tracker = AttributionTracker::new();
    let old_content = "line 2\n";
    let new_content = "line 1\nline 2\n";

    let old_attrs = vec![Attribution::new(0, 7, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 2000)
        .unwrap();

    // New content at start should be attributed to current author
    assert!(
        new_attrs
            .iter()
            .any(|a| a.author_id == "current-author" && a.start == 0)
    );
    // Old content should be shifted and preserved
    assert!(
        new_attrs
            .iter()
            .any(|a| a.author_id == "ai-1" && a.start > 0)
    );
}

#[test]
fn test_tracker_simple_deletion_at_end() {
    // Test deleting content at the end
    let tracker = AttributionTracker::new();
    let old_content = "line 1\nline 2\n";
    let new_content = "line 1\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "ai-2".to_string(), 2000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 3000)
        .unwrap();

    // Should preserve first attribution only
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-1"));
    // Deleted content attribution should be gone or marked with deletion
    // There might be a marker attribution for the deletion
    assert!(
        new_attrs
            .iter()
            .any(|a| a.author_id == "current-author" || a.author_id == "ai-1")
    );
}

#[test]
fn test_tracker_simple_deletion_at_start() {
    // Test deleting content at the start
    let tracker = AttributionTracker::new();
    let old_content = "line 1\nline 2\n";
    let new_content = "line 2\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "ai-2".to_string(), 2000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 3000)
        .unwrap();

    // Should preserve second attribution, shifted to start
    assert!(
        new_attrs
            .iter()
            .any(|a| a.author_id == "ai-2" || a.author_id == "current-author")
    );
}

#[test]
fn test_tracker_modification_in_middle() {
    // Test modifying content in the middle
    let tracker = AttributionTracker::new();
    let old_content = "line 1\nline 2\nline 3\n";
    let new_content = "line 1\nmodified\nline 3\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "ai-2".to_string(), 2000),
        Attribution::new(14, 21, "ai-3".to_string(), 3000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 4000)
        .unwrap();

    // Should preserve first and last attributions
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-1"));
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-3"));
    // Middle should be attributed to current author
    assert!(new_attrs.iter().any(|a| a.author_id == "current-author"));
}

// =============================================================================
// Configuration Tests
// =============================================================================

#[test]
fn test_tracker_with_default_config() {
    // Test creating tracker with default configuration
    let config = AttributionConfig::default();
    let tracker = AttributionTracker::with_config(config);

    // Just verify it works with default config
    let old_content = "test\n";
    let new_content = "test modified\n";
    let old_attrs = vec![Attribution::new(0, 5, "ai-1".to_string(), 1000)];

    let result = tracker.update_attributions(old_content, new_content, &old_attrs, "current", 2000);
    assert!(result.is_ok());
}

// =============================================================================
// Edge Cases and Error Handling
// =============================================================================

#[test]
fn test_tracker_empty_old_content() {
    // Test with empty old content (new file)
    let tracker = AttributionTracker::new();
    let old_content = "";
    let new_content = "new file content\n";
    let old_attrs = vec![];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "author", 1000)
        .unwrap();

    assert!(!new_attrs.is_empty());
    assert!(new_attrs.iter().all(|a| a.author_id == "author"));
}

#[test]
fn test_tracker_empty_new_content() {
    // Test with empty new content (file deletion)
    let tracker = AttributionTracker::new();
    let old_content = "file content\n";
    let new_content = "";
    let old_attrs = vec![Attribution::new(0, 13, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "author", 2000)
        .unwrap();

    // Should have no or minimal attributions for empty file
    assert!(new_attrs.is_empty() || new_attrs.iter().all(|a| a.is_empty()));
}

#[test]
fn test_tracker_both_empty() {
    // Test with both old and new content empty
    let tracker = AttributionTracker::new();
    let old_content = "";
    let new_content = "";
    let old_attrs = vec![];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "author", 1000)
        .unwrap();

    assert!(new_attrs.is_empty());
}

#[test]
fn test_tracker_single_character_changes() {
    // Test single character insertions and deletions
    let tracker = AttributionTracker::new();
    let old_content = "abc\n";
    let new_content = "abxc\n";

    let old_attrs = vec![Attribution::new(0, 4, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_nested_structures() {
    // Test code with nested structures
    let tracker = AttributionTracker::new();
    let old_content = "fn outer() {\n    fn inner() {\n        code\n    }\n}\n";
    let new_content = "fn outer() {\n    fn inner() {\n        modified\n    }\n}\n";

    let old_attrs = vec![Attribution::new(0, 48, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(new_attrs.iter().any(|a| a.author_id == "current"));
}

#[test]
fn test_attribution_with_merge_conflict_markers() {
    // Test handling merge conflict markers as regular text
    let tracker = AttributionTracker::new();
    let old_content = "normal line\n";
    let new_content = "<<<<<<< HEAD\nnormal line\n=======\nother line\n>>>>>>> branch\n";

    let old_attrs = vec![Attribution::new(0, 12, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_regex_like_patterns() {
    // Test content with regex-like patterns
    let tracker = AttributionTracker::new();
    let old_content = "pattern: [a-z]+\n";
    let new_content = "pattern: [a-zA-Z]+\n";

    let old_attrs = vec![Attribution::new(0, 16, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_json_like_content() {
    // Test JSON-like structured content
    let tracker = AttributionTracker::new();
    let old_content = r#"{"key": "value"}"#.to_string() + "\n";
    let new_content = r#"{"key": "new_value", "extra": true}"#.to_string() + "\n";

    let old_attrs = vec![Attribution::new(0, 17, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(&old_content, &new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_url_like_content() {
    // Test URLs and paths
    let tracker = AttributionTracker::new();
    let old_content = "https://example.com/path\n";
    let new_content = "https://example.com/newpath?query=1\n";

    let old_attrs = vec![Attribution::new(0, 25, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

crate::reuse_tests_in_worktree!(
    test_tracker_no_changes_preserves_attributions,
    test_tracker_simple_addition_at_end,
    test_tracker_simple_addition_at_start,
    test_tracker_simple_deletion_at_end,
    test_tracker_simple_deletion_at_start,
    test_tracker_modification_in_middle,
    test_tracker_with_default_config,
    test_tracker_empty_old_content,
    test_tracker_empty_new_content,
    test_tracker_both_empty,
    test_tracker_single_character_changes,
    test_tracker_nested_structures,
    test_attribution_with_merge_conflict_markers,
    test_tracker_regex_like_patterns,
    test_tracker_json_like_content,
    test_tracker_url_like_content,
);
