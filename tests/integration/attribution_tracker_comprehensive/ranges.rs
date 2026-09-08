use super::{Attribution, LineAttribution};

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

crate::reuse_tests_in_worktree!(
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
);
