use super::{Attribution, AttributionTracker};

// =============================================================================
// Large File Performance Tests
// =============================================================================

#[test]
fn test_tracker_large_file_many_lines() {
    // Test performance with a large number of lines
    let tracker = AttributionTracker::new();

    // Generate 1000 lines
    let mut old_lines = Vec::new();
    let mut old_attrs = Vec::new();
    let mut pos = 0;
    for i in 0..1000 {
        let line = format!("line {}\n", i);
        let len = line.len();
        old_lines.push(line);
        old_attrs.push(Attribution::new(
            pos,
            pos + len,
            format!("ai-{}", i % 10),
            1000,
        ));
        pos += len;
    }
    let old_content = old_lines.join("");

    // Modify a few lines in the middle
    let mut new_lines = old_lines.clone();
    new_lines[500] = "modified line 500\n".to_string();
    new_lines[501] = "modified line 501\n".to_string();
    let new_content = new_lines.join("");

    let result =
        tracker.update_attributions(&old_content, &new_content, &old_attrs, "current", 2000);
    assert!(result.is_ok());

    let new_attrs = result.unwrap();
    // Should have roughly the same number of attributions
    assert!(new_attrs.len() > 900);
}

#[test]
fn test_tracker_large_file_long_lines() {
    // Test performance with very long lines
    let tracker = AttributionTracker::new();

    // Generate a file with a few very long lines
    let long_line = "x".repeat(10000);
    let old_content = format!("{}\n{}\n", long_line, long_line);
    let new_content = format!("{}\nmodified\n", long_line);

    let old_attrs = vec![
        Attribution::new(0, 10001, "ai-1".to_string(), 1000),
        Attribution::new(10001, 20002, "ai-2".to_string(), 2000),
    ];

    let result =
        tracker.update_attributions(&old_content, &new_content, &old_attrs, "current", 3000);
    assert!(result.is_ok());
}

#[test]
fn test_tracker_many_small_changes() {
    // Test many small changes throughout a file
    let tracker = AttributionTracker::new();

    let old_content = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
    let new_content = "A\nb\nC\nd\nE\nf\nG\nh\nI\nj\n";

    let old_attrs = vec![
        Attribution::new(0, 2, "ai-1".to_string(), 1000),
        Attribution::new(2, 4, "ai-2".to_string(), 1000),
        Attribution::new(4, 6, "ai-3".to_string(), 1000),
        Attribution::new(6, 8, "ai-4".to_string(), 1000),
        Attribution::new(8, 10, "ai-5".to_string(), 1000),
        Attribution::new(10, 12, "ai-6".to_string(), 1000),
        Attribution::new(12, 14, "ai-7".to_string(), 1000),
        Attribution::new(14, 16, "ai-8".to_string(), 1000),
        Attribution::new(16, 18, "ai-9".to_string(), 1000),
        Attribution::new(18, 20, "ai-10".to_string(), 1000),
    ];

    let result = tracker.update_attributions(old_content, new_content, &old_attrs, "current", 2000);
    assert!(result.is_ok());
}

#[test]
fn test_tracker_very_long_single_line() {
    // Test handling of a very long single line
    let tracker = AttributionTracker::new();
    let old_content = "x".repeat(100000) + "\n";
    let new_content = "x".repeat(50000) + "y" + &"x".repeat(50000) + "\n";

    let old_attrs = vec![Attribution::new(0, 100001, "ai-1".to_string(), 1000)];

    let result =
        tracker.update_attributions(&old_content, &new_content, &old_attrs, "current", 2000);
    assert!(result.is_ok());
}

#[test]
fn test_tracker_complete_file_replacement() {
    // Test completely replacing file content
    let tracker = AttributionTracker::new();
    let old_content = "old content line 1\nold content line 2\n";
    let new_content = "completely\ndifferent\ncontent\n";

    let old_attrs = vec![
        Attribution::new(0, 19, "ai-1".to_string(), 1000),
        Attribution::new(19, 38, "ai-2".to_string(), 2000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 3000)
        .unwrap();

    // All new content should be attributed to current author
    assert!(new_attrs.iter().any(|a| a.author_id == "current"));
}

#[test]
fn test_tracker_alternating_small_edits() {
    // Test alternating character-level edits
    let tracker = AttributionTracker::new();
    let old_content = "a b c d e\n";
    let new_content = "A B C D E\n";

    let old_attrs = vec![Attribution::new(0, 10, "ai-1".to_string(), 1000)];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 2000)
        .unwrap();

    assert!(!new_attrs.is_empty());
}

#[test]
fn test_tracker_massive_insertion() {
    // Test inserting a large block of text
    let tracker = AttributionTracker::new();
    let old_content = "start\nend\n";
    let mut middle = String::new();
    for i in 0..100 {
        middle.push_str(&format!("inserted line {}\n", i));
    }
    let new_content = format!("start\n{}end\n", middle);

    let old_attrs = vec![
        Attribution::new(0, 6, "ai-1".to_string(), 1000),
        Attribution::new(6, 10, "ai-2".to_string(), 2000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, &new_content, &old_attrs, "current", 3000)
        .unwrap();

    assert!(new_attrs.iter().any(|a| a.author_id == "ai-1"));
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-2"));
    assert!(new_attrs.iter().any(|a| a.author_id == "current"));
}

#[test]
fn test_tracker_massive_deletion() {
    // Test deleting a large block of text
    let tracker = AttributionTracker::new();
    let mut middle = String::new();
    for i in 0..100 {
        middle.push_str(&format!("to be deleted {}\n", i));
    }
    let old_content = format!("start\n{}end\n", middle);
    let new_content = "start\nend\n";

    let old_attrs = vec![
        Attribution::new(0, 6, "ai-1".to_string(), 1000),
        Attribution::new(6, old_content.len() - 4, "ai-2".to_string(), 2000),
        Attribution::new(
            old_content.len() - 4,
            old_content.len(),
            "ai-3".to_string(),
            3000,
        ),
    ];

    let new_attrs = tracker
        .update_attributions(&old_content, new_content, &old_attrs, "current", 4000)
        .unwrap();

    assert!(new_attrs.iter().any(|a| a.author_id == "ai-1"));
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-3"));
}

#[test]
fn test_attribution_consistency_multiple_rounds() {
    // Test that multiple rounds of attribution produce consistent results
    let tracker = AttributionTracker::new();
    let content1 = "line 1\n";
    let content2 = "line 1\nline 2\n";
    let content3 = "line 1\nline 2\nline 3\n";

    let attrs1 = tracker
        .update_attributions("", content1, &[], "author1", 1000)
        .unwrap();

    let attrs2 = tracker
        .update_attributions(content1, content2, &attrs1, "author2", 2000)
        .unwrap();

    let attrs3 = tracker
        .update_attributions(content2, content3, &attrs2, "author3", 3000)
        .unwrap();

    // Should have attributions from all three authors
    assert!(attrs3.iter().any(|a| a.author_id == "author1"));
    assert!(attrs3.iter().any(|a| a.author_id == "author2"));
    assert!(attrs3.iter().any(|a| a.author_id == "author3"));
}

#[test]
fn test_tracker_symmetric_changes() {
    // Test symmetric changes (same edit at multiple locations)
    let tracker = AttributionTracker::new();
    let old_content = "foo\nbar\nfoo\n";
    let new_content = "FOO\nbar\nFOO\n";

    let old_attrs = vec![
        Attribution::new(0, 4, "ai-1".to_string(), 1000),
        Attribution::new(4, 8, "ai-2".to_string(), 2000),
        Attribution::new(8, 12, "ai-3".to_string(), 3000),
    ];

    let new_attrs = tracker
        .update_attributions(old_content, new_content, &old_attrs, "current", 4000)
        .unwrap();

    // Middle line should be preserved
    assert!(new_attrs.iter().any(|a| a.author_id == "ai-2"));
}

#[test]
fn test_tracker_progressive_file_growth() {
    // Test progressive file growth over multiple edits
    let tracker = AttributionTracker::new();

    let mut content = "initial\n".to_string();
    let mut attrs = tracker
        .update_attributions("", &content, &[], "author0", 1000)
        .unwrap();

    // Add lines progressively
    for i in 1..10 {
        let new_content = format!("{}line {}\n", content, i);
        attrs = tracker
            .update_attributions(
                &content,
                &new_content,
                &attrs,
                &format!("author{}", i),
                1000 + i as u128 * 100,
            )
            .unwrap();
        content = new_content;
    }

    // Should have attributions from multiple authors
    assert!(attrs.iter().any(|a| a.author_id == "author0"));
    assert!(attrs.iter().any(|a| a.author_id.starts_with("author")));
    assert!(attrs.len() >= 10);
}

crate::reuse_tests_in_worktree!(
    test_tracker_large_file_many_lines,
    test_tracker_large_file_long_lines,
    test_tracker_many_small_changes,
    test_tracker_very_long_single_line,
    test_tracker_complete_file_replacement,
    test_tracker_alternating_small_edits,
    test_tracker_massive_insertion,
    test_tracker_massive_deletion,
    test_attribution_consistency_multiple_rounds,
    test_tracker_symmetric_changes,
    test_tracker_progressive_file_growth,
);
