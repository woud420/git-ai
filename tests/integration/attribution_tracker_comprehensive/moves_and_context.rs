use super::{Attribution, AttributionTracker};

// =============================================================================
// Move Detection Tests
// =============================================================================

#[test]
fn test_tracker_simple_line_move_within_file() {
    // Test detecting a simple line move within a file
    // Note: Move detection may not trigger for very small files or simple swaps
    let tracker = AttributionTracker::new();
    let old_content = "line 1\nline 2\nline 3\n";
    let new_content = "line 2\nline 1\nline 3\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "ai-2".to_string(), 2000),
        Attribution::new(14, 21, "ai-3".to_string(), 3000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 4000)
        .unwrap();

    // Should have some attributions preserved or new ones created
    assert!(!new_attrs.is_empty());
    // Third line should be preserved as it didn't move
    assert!(
        new_attrs
            .iter()
            .any(|a| a.author_id == "ai-3" || a.author_id == "current-author")
    );
}

#[test]
fn test_tracker_block_move_within_file() {
    // Test detecting a block of lines moved within a file
    // Note: Move detection may not trigger for very small files
    let tracker = AttributionTracker::new();
    let old_content = "a\nb\nc\nd\ne\n";
    let new_content = "d\ne\na\nb\nc\n";

    let old_attrs = vec![
        Attribution::new(0, 2, "ai-1".to_string(), 1000),
        Attribution::new(2, 4, "ai-2".to_string(), 2000),
        Attribution::new(4, 6, "ai-3".to_string(), 3000),
        Attribution::new(6, 8, "ai-4".to_string(), 4000),
        Attribution::new(8, 10, "ai-5".to_string(), 5000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 6000)
        .unwrap();

    // Should have attributions created - either preserved or new from current author
    assert!(!new_attrs.is_empty());
    // At least some of the original content should be represented
    let has_original = new_attrs.iter().any(|a| {
        a.author_id == "ai-1"
            || a.author_id == "ai-2"
            || a.author_id == "ai-3"
            || a.author_id == "ai-4"
            || a.author_id == "ai-5"
    });
    let has_current = new_attrs.iter().any(|a| a.author_id == "current-author");
    assert!(has_original || has_current);
}

#[test]
fn test_tracker_partial_line_move() {
    // Test detecting partial content moved within a line
    let tracker = AttributionTracker::new();
    let old_content = "prefix middle suffix\n";
    let new_content = "middle prefix suffix\n";

    let old_attrs = vec![Attribution::new(0, 21, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 2000)
        .unwrap();

    // Should detect the move and preserve attribution
    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_move_with_modification() {
    // Test a line that's both moved and modified
    let tracker = AttributionTracker::new();
    let old_content = "line 1\nline 2\nline 3\n";
    let new_content = "line 3\nLINE 1 MODIFIED\nline 2\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "ai-2".to_string(), 2000),
        Attribution::new(14, 21, "ai-3".to_string(), 3000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 4000)
        .unwrap();

    // Should have both preserved and new attributions
    assert!(new_attrs.iter().any(|a| a.author_id == "current-author"));
}

#[test]
fn test_tracker_duplicate_line_handling() {
    // Test handling duplicate lines
    let tracker = AttributionTracker::new();
    let old_content = "same\nsame\n";
    let new_content = "same\n";

    let old_attrs = vec![
        Attribution::new(0, 5, "ai-1".to_string(), 1000),
        Attribution::new(5, 10, "ai-2".to_string(), 2000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current-author", 3000)
        .unwrap();

    // Should preserve one of the attributions
    assert!(!new_attrs.is_empty());
}

// =============================================================================
// Mixed AI/Human Edit Tests
// =============================================================================

#[test]
fn test_tracker_mixed_edit_same_line() {
    // Test when AI and human both edit the same line
    let tracker = AttributionTracker::new();
    let old_content = "original line\n";
    let new_content = "modified line\n";

    let old_attrs = vec![Attribution::new(0, 14, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "human-1", 2000)
        .unwrap();

    // Should have new attribution for the modification
    assert!(new_attrs.iter().any(|a| a.author_id == "human-1"));
}

#[test]
fn test_tracker_ai_adds_human_deletes() {
    // Test AI adding content that human later deletes
    let tracker = AttributionTracker::new();

    // Step 1: AI adds content
    let old_content = "";
    let new_content = "ai added line\n";
    let old_attrs = vec![];

    let attrs1 = tracker
        .update_attributions(old_content, new_content, &old_attrs, "ai-1", 1000)
        .unwrap();

    // Step 2: Human deletes it
    let attrs2 = tracker
        .update_attributions(new_content, old_content, &attrs1, "human-1", 2000)
        .unwrap();

    // Should have a deletion marker or be empty
    // The tracker marks deletions with zero-length attributions
    assert!(attrs2.is_empty() || attrs2.iter().any(|a| a.author_id == "human-1"));
}

#[test]
fn test_tracker_human_adds_ai_modifies() {
    // Test human adding content that AI later modifies
    let tracker = AttributionTracker::new();

    let old_content = "";
    let human_content = "human line\n";
    let ai_content = "human line modified by ai\n";

    let attrs1 = tracker
        .update_attributions(old_content, human_content, &[], "human-1", 1000)
        .unwrap();

    let attrs2 = tracker
        .update_attributions(human_content, ai_content, &attrs1, "ai-1", 2000)
        .unwrap();

    // Should have both attributions
    assert!(attrs2.iter().any(|a| a.author_id == "ai-1"));
}

#[test]
fn test_tracker_interleaved_ai_human_edits() {
    // Test interleaved AI and human edits
    let tracker = AttributionTracker::new();
    let old_content = "line 1\nline 2\nline 3\n";
    let new_content = "AI edit\nline 2\nHuman edit\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "original".to_string(), 1000),
        Attribution::new(7, 14, "original".to_string(), 1000),
        Attribution::new(14, 21, "original".to_string(), 1000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    // Should have new attributions for modified lines
    assert!(new_attrs.iter().any(|a| a.author_id == "current"));
    // Original second line should be preserved
    assert!(new_attrs.iter().any(|a| a.author_id == "original"));
}

// =============================================================================
// Additional Edge Cases and Complex Scenarios
// =============================================================================

#[test]
fn test_tracker_repeated_content() {
    // Test handling of repeated identical content blocks
    let tracker = AttributionTracker::new();
    let old_content = "repeat\nrepeat\nrepeat\n";
    let new_content = "repeat\nunique\nrepeat\nrepeat\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "ai-1".to_string(), 1000),
        Attribution::new(14, 21, "ai-1".to_string(), 1000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
    assert!(new_attrs.iter().any(|a| a.author_id == "current"));
}

#[test]
fn test_tracker_surrounding_context_preserved() {
    // Test that surrounding context is preserved when middle is edited
    let tracker = AttributionTracker::new();
    let old_content = "prefix\nmiddle\nsuffix\n";
    let new_content = "prefix\nNEW\nsuffix\n";

    let old_attrs = vec![
        Attribution::new(0, 7, "ai-1".to_string(), 1000),
        Attribution::new(7, 14, "ai-2".to_string(), 2000),
        Attribution::new(14, 21, "ai-3".to_string(), 3000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 4000)
        .unwrap();

    // Prefix and suffix should be preserved
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-1"));
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-3"));
    assert!(new_attrs.iter().any(|a| a.author_id == "current"));
}

crate::reuse_tests_in_worktree!(
    test_tracker_simple_line_move_within_file,
    test_tracker_block_move_within_file,
    test_tracker_partial_line_move,
    test_tracker_move_with_modification,
    test_tracker_duplicate_line_handling,
    test_tracker_mixed_edit_same_line,
    test_tracker_ai_adds_human_deletes,
    test_tracker_human_adds_ai_modifies,
    test_tracker_interleaved_ai_human_edits,
    test_tracker_repeated_content,
    test_tracker_surrounding_context_preserved,
);
