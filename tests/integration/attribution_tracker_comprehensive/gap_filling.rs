use super::{Attribution, AttributionTracker};

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

crate::reuse_tests_in_worktree!(
    test_attribute_unattributed_fills_gaps,
    test_attribute_unattributed_no_gaps,
    test_attribute_unattributed_multiple_gaps,
    test_attribute_unattributed_empty_content,
    test_attribute_unattributed_overlapping_attrs,
);
