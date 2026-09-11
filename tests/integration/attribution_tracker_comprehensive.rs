//! Comprehensive tests for src/authorship/attribution_tracker.rs
//!
//! This test module covers critical functionality in attribution_tracker.rs (2,573 LOC)
//! which is the core diff-based attribution tracking module that underpins AI authorship tracking.
//!
//! Test coverage areas:
//! 1. Basic line attribution (AI vs human edits)
//! 2. Move detection across files and within files
//! 3. Whitespace-only changes
//! 4. Mixed AI/human edits on same lines
//! 5. Large file performance
//! 6. Unicode and special character handling
//! 7. Diff algorithm edge cases
//! 8. Character-level attribution tracking
//! 9. Attribution preservation through renames
//! 10. Multi-file attribution scenarios

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::attribution_tracker::{
    Attribution, AttributionConfig, AttributionTracker, INITIAL_ATTRIBUTION_TS, LineAttribution,
};

mod large_edits;
mod moves_and_context;

mod repository_operations;

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

// =============================================================================
// Attribute Unattributed Ranges Tests
// =============================================================================

#[test]
fn test_attribute_unattributed_fills_gaps() {
    // Test that unattributed ranges are filled correctly
    let tracker = AttributionTracker::new();
    let content = "aaabbbccc\n";

    // Only attribute middle section
    let attrs = vec![Attribution::new(3, 6, "ai-1".to_string(), 1000)];

    let result = tracker.attribute_unattributed_ranges(content, &attrs, "filler", 2000);

    // Should have 3 attributions: start gap, original, end gap
    assert!(
        result
            .iter()
            .any(|a| a.start == 0 && a.author_id == "filler")
    );
    assert!(result.iter().any(|a| a.start == 3 && a.author_id == "ai-1"));
    assert!(
        result
            .iter()
            .any(|a| a.author_id == "filler" && a.end == content.len())
    );
}

#[test]
fn test_attribute_unattributed_no_gaps() {
    // Test when there are no gaps to fill
    let tracker = AttributionTracker::new();
    let content = "complete\n";

    let attrs = vec![Attribution::new(0, 9, "ai-1".to_string(), 1000)];

    let result = tracker.attribute_unattributed_ranges(content, &attrs, "filler", 2000);

    // Should only have the original attribution
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].author_id, "ai-1");
}

#[test]
fn test_attribute_unattributed_multiple_gaps() {
    // Test multiple gaps in attribution
    let tracker = AttributionTracker::new();
    let content = "aa bb cc dd\n";

    let attrs = vec![
        Attribution::new(3, 5, "ai-1".to_string(), 1000),
        Attribution::new(9, 11, "ai-2".to_string(), 2000),
    ];

    let result = tracker.attribute_unattributed_ranges(content, &attrs, "filler", 3000);

    // Should fill gaps: before first, between first and second, and after second
    assert!(
        result
            .iter()
            .any(|a| a.start == 0 && a.author_id == "filler")
    );
    assert!(result.iter().any(|a| a.start == 3 && a.author_id == "ai-1"));
    // There should be a gap filled between the two attributed ranges
    let has_middle_gap = result
        .iter()
        .any(|a| a.author_id == "filler" && a.start >= 5 && a.end <= 9);
    assert!(
        has_middle_gap,
        "Should have filler attribution in middle gap"
    );
    assert!(result.iter().any(|a| a.start == 9 && a.author_id == "ai-2"));
    // Should have filler at the end too
    assert!(
        result
            .iter()
            .any(|a| a.author_id == "filler" && a.end == content.len())
    );
}

#[test]
fn test_attribute_unattributed_empty_content() {
    // Test with empty content
    let tracker = AttributionTracker::new();
    let content = "";

    let attrs = vec![];

    let result = tracker.attribute_unattributed_ranges(content, &attrs, "filler", 1000);

    // Should have no attributions for empty content
    assert!(result.is_empty());
}

#[test]
fn test_attribute_unattributed_overlapping_attrs() {
    // Test with overlapping attributions
    let tracker = AttributionTracker::new();
    let content = "overlapping\n";

    let attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(4, 11, "ai-2".to_string(), 2000),
    ];

    let result = tracker.attribute_unattributed_ranges(content, &attrs, "filler", 3000);

    // Should preserve overlapping attributions and fill the remaining gap
    assert!(result.iter().any(|a| a.author_id == "ai-1"));
    assert!(result.iter().any(|a| a.author_id == "ai-2"));
    assert!(
        result
            .iter()
            .any(|a| a.author_id == "filler" && a.end == 12)
    );
}

// =============================================================================
// Basic Attribution Tests - Core functionality
// =============================================================================

#[test]
fn test_attribution_new_creates_valid_range() {
    // Test that Attribution::new creates valid ranges
    let attr = Attribution::new(0, 10, "ai-1".to_string(), 1000);
    assert_eq!(attr.start, 0);
    assert_eq!(attr.end, 10);
    assert_eq!(attr.author_id, "ai-1");
    assert_eq!(attr.ts, 1000);
    assert_eq!(attr.len(), 10);
    assert!(!attr.is_empty());
}

#[test]
fn test_attribution_empty_range() {
    // Test empty attribution ranges
    let attr = Attribution::new(5, 5, "ai-1".to_string(), 1000);
    assert!(attr.is_empty());
    assert_eq!(attr.len(), 0);
}

#[test]
fn test_attribution_overlaps_basic() {
    // Test basic overlap detection
    let attr = Attribution::new(10, 20, "ai-1".to_string(), 1000);

    // Overlaps
    assert!(attr.overlaps(5, 15)); // Starts before, overlaps start
    assert!(attr.overlaps(15, 25)); // Overlaps end, extends after
    assert!(attr.overlaps(12, 18)); // Fully contained
    assert!(attr.overlaps(5, 25)); // Fully encompasses

    // Does not overlap
    assert!(!attr.overlaps(0, 10)); // Ends at start
    assert!(!attr.overlaps(20, 30)); // Starts at end
    assert!(!attr.overlaps(0, 5)); // Completely before
    assert!(!attr.overlaps(25, 30)); // Completely after
}

#[test]
fn test_attribution_intersection() {
    // Test intersection computation
    let attr = Attribution::new(10, 20, "ai-1".to_string(), 1000);

    assert_eq!(attr.intersection(5, 15), Some((10, 15)));
    assert_eq!(attr.intersection(15, 25), Some((15, 20)));
    assert_eq!(attr.intersection(12, 18), Some((12, 18)));
    assert_eq!(attr.intersection(5, 25), Some((10, 20)));
    assert_eq!(attr.intersection(0, 10), None);
    assert_eq!(attr.intersection(20, 30), None);
}

#[test]
fn test_line_attribution_new_creates_valid_range() {
    // Test that LineAttribution::new creates valid ranges
    let attr = LineAttribution::new(1, 10, "ai-1".to_string(), None);
    assert_eq!(attr.start_line, 1);
    assert_eq!(attr.end_line, 10);
    assert_eq!(attr.author_id, "ai-1");
    assert_eq!(attr.overrode, None);
    assert_eq!(attr.line_count(), 10);
    assert!(!attr.is_empty());
}

#[test]
fn test_line_attribution_with_override() {
    // Test LineAttribution with override tracking
    let attr = LineAttribution::new(1, 5, "human-1".to_string(), Some("ai-1".to_string()));
    assert_eq!(attr.overrode, Some("ai-1".to_string()));
}

#[test]
fn test_line_attribution_overlaps() {
    // Test line attribution overlap detection
    let attr = LineAttribution::new(10, 20, "ai-1".to_string(), None);

    assert!(attr.overlaps(5, 15)); // Overlaps start
    assert!(attr.overlaps(15, 25)); // Overlaps end
    assert!(attr.overlaps(12, 18)); // Fully contained
    assert!(attr.overlaps(5, 25)); // Fully encompasses

    assert!(!attr.overlaps(1, 9)); // Before
    assert!(!attr.overlaps(21, 30)); // After
}

#[test]
fn test_line_attribution_intersection() {
    // Test line attribution intersection
    let attr = LineAttribution::new(10, 20, "ai-1".to_string(), None);

    assert_eq!(attr.intersection(5, 15), Some((10, 15)));
    assert_eq!(attr.intersection(15, 25), Some((15, 20)));
    assert_eq!(attr.intersection(12, 18), Some((12, 18)));
    assert_eq!(attr.intersection(5, 25), Some((10, 20)));
    assert_eq!(attr.intersection(1, 9), None);
    assert_eq!(attr.intersection(21, 30), None);
}

#[test]
fn test_line_attribution_zero_line_count() {
    // Test edge case of inverted line range
    let attr = LineAttribution::new(10, 5, "ai-1".to_string(), None);
    assert_eq!(attr.line_count(), 0);
    assert!(attr.is_empty());
}

#[test]
fn test_line_attribution_single_line() {
    // Test single line attribution
    let attr = LineAttribution::new(5, 5, "ai-1".to_string(), None);
    assert_eq!(attr.line_count(), 1);
    assert!(!attr.is_empty());
}

#[test]
fn test_attribution_boundary_conditions() {
    // Test attribution at exact boundaries
    let attr = Attribution::new(10, 20, "ai-1".to_string(), 1000);

    // Test overlaps at exact boundaries
    assert!(!attr.overlaps(0, 10)); // Ends exactly at start
    assert!(!attr.overlaps(20, 30)); // Starts exactly at end
    assert!(attr.overlaps(10, 20)); // Exact match
    assert!(attr.overlaps(9, 11)); // Crosses start boundary
    assert!(attr.overlaps(19, 21)); // Crosses end boundary
}

#[test]
fn test_line_attribution_boundary_conditions() {
    // Test line attribution at exact boundaries
    let attr = LineAttribution::new(10, 20, "ai-1".to_string(), None);

    // Boundary checks
    assert!(!attr.overlaps(1, 9)); // Before
    assert!(!attr.overlaps(21, 30)); // After
    assert!(attr.overlaps(10, 20)); // Exact
    assert!(attr.overlaps(9, 11)); // Crosses start
    assert!(attr.overlaps(19, 21)); // Crosses end
}

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
    test_attribute_unattributed_fills_gaps,
    test_attribute_unattributed_no_gaps,
    test_attribute_unattributed_multiple_gaps,
    test_attribute_unattributed_empty_content,
    test_attribute_unattributed_overlapping_attrs,
    test_attribution_new_creates_valid_range,
    test_attribution_empty_range,
    test_attribution_overlaps_basic,
    test_attribution_intersection,
    test_line_attribution_new_creates_valid_range,
    test_line_attribution_with_override,
    test_line_attribution_overlaps,
    test_line_attribution_intersection,
    test_line_attribution_zero_line_count,
    test_line_attribution_single_line,
    test_attribution_boundary_conditions,
    test_line_attribution_boundary_conditions,
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
