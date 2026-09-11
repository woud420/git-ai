use super::*;

#[test]
fn parse_commit_metric_metadata_output_reads_subject_body_and_timestamps() {
    let metadata = parse_commit_metric_metadata_output(concat!(
        "Subject line",
        "\0",
        "Body line one\n\nBody line two",
        "\0",
        "1704067200",
        "\0",
        "1704067260\n"
    ));

    assert_eq!(metadata.subject, Some("Subject line".to_string()));
    assert_eq!(
        metadata.body,
        Some("Body line one\n\nBody line two".to_string())
    );
    assert_eq!(metadata.author_ts, Some(1_704_067_200));
    assert_eq!(metadata.commit_ts, Some(1_704_067_260));
}

#[test]
fn parse_commit_metric_metadata_output_uses_null_body_for_empty_body() {
    let metadata = parse_commit_metric_metadata_output(concat!(
        "Subject line",
        "\0",
        "",
        "\0",
        "1704067200",
        "\0",
        "1704067260\n"
    ));

    assert_eq!(metadata.subject, Some("Subject line".to_string()));
    assert_eq!(metadata.body, None);
    assert_eq!(metadata.author_ts, Some(1_704_067_200));
    assert_eq!(metadata.commit_ts, Some(1_704_067_260));
}

#[test]
fn test_count_line_ranges_handles_scattered_and_contiguous_lines() {
    assert_eq!(count_line_ranges(&[]), 0);
    assert_eq!(count_line_ranges(&[1]), 1);
    assert_eq!(count_line_ranges(&[1, 2, 3]), 1);
    assert_eq!(count_line_ranges(&[1, 3, 5]), 3);
    // Includes unsorted and duplicate values.
    assert_eq!(count_line_ranges(&[5, 3, 3, 4, 10]), 2);
    assert_eq!(
        count_line_ranges(&[u32::MAX, 0, 1, u32::MAX, u32::MAX - 1]),
        2
    );
}

#[test]
fn test_should_skip_expensive_post_commit_stats_thresholds() {
    let below_threshold = StatsCostEstimate {
        files_with_additions: STATS_SKIP_MAX_FILES_WITH_ADDITIONS - 1,
        added_lines: STATS_SKIP_MAX_ADDED_LINES - 1,
        hunk_ranges: STATS_SKIP_MAX_HUNKS - 1,
        deleted_lines: STATS_SKIP_MAX_DELETED_LINES - 1,
    };
    assert!(!should_skip_expensive_post_commit_stats(&below_threshold));

    let by_hunks = StatsCostEstimate {
        files_with_additions: 1,
        added_lines: 1,
        hunk_ranges: STATS_SKIP_MAX_HUNKS,
        deleted_lines: 0,
    };
    assert!(should_skip_expensive_post_commit_stats(&by_hunks));

    let by_added_lines = StatsCostEstimate {
        files_with_additions: 1,
        added_lines: STATS_SKIP_MAX_ADDED_LINES,
        hunk_ranges: 1,
        deleted_lines: 0,
    };
    assert!(should_skip_expensive_post_commit_stats(&by_added_lines));

    let by_files = StatsCostEstimate {
        files_with_additions: STATS_SKIP_MAX_FILES_WITH_ADDITIONS,
        added_lines: 1,
        hunk_ranges: 1,
        deleted_lines: 0,
    };
    assert!(should_skip_expensive_post_commit_stats(&by_files));

    let by_deleted_lines = StatsCostEstimate {
        files_with_additions: 0,
        added_lines: 0,
        hunk_ranges: 0,
        deleted_lines: STATS_SKIP_MAX_DELETED_LINES,
    };
    assert!(should_skip_expensive_post_commit_stats(&by_deleted_lines));
}

#[test]
fn test_count_line_ranges_single_element() {
    assert_eq!(count_line_ranges(&[42]), 1);
}

#[test]
fn test_count_line_ranges_all_contiguous() {
    assert_eq!(count_line_ranges(&[1, 2, 3, 4, 5]), 1);
}

#[test]
fn test_count_line_ranges_all_scattered() {
    assert_eq!(count_line_ranges(&[1, 10, 20, 30]), 4);
}

#[test]
fn test_count_line_ranges_duplicates() {
    assert_eq!(count_line_ranges(&[5, 5, 5]), 1);
}

#[test]
fn test_count_line_ranges_unsorted() {
    // After sort+dedup: [1, 2, 5, 6, 10] -> ranges: [1,2], [5,6], [10]
    assert_eq!(count_line_ranges(&[10, 5, 6, 1, 2]), 3);
}

#[test]
fn test_metric_tool_model_breakdown_filters_mock_ai() {
    use crate::operations::authorship::stats::{CommitStats, ToolModelHeadlineStats};

    let mut tool_model_breakdown = std::collections::BTreeMap::new();
    tool_model_breakdown.insert(
        "mock_ai::unknown".to_string(),
        ToolModelHeadlineStats {
            ai_additions: 4,
            ai_accepted: 3,
        },
    );
    tool_model_breakdown.insert(
        "codex::gpt-5".to_string(),
        ToolModelHeadlineStats {
            ai_additions: 6,
            ai_accepted: 5,
        },
    );
    let stats = CommitStats {
        ai_additions: 10,
        ai_accepted: 8,
        tool_model_breakdown,
        ..Default::default()
    };

    let result = metric_tool_model_breakdown(&stats).unwrap();

    assert_eq!(result.tool_model_pairs, vec!["all", "codex::gpt-5"]);
    assert_eq!(result.ai_additions, vec![6, 6]);
    assert_eq!(result.ai_accepted, vec![5, 5]);
}

#[test]
fn test_metric_tool_model_breakdown_skips_mock_only() {
    use crate::operations::authorship::stats::{CommitStats, ToolModelHeadlineStats};

    let mut tool_model_breakdown = std::collections::BTreeMap::new();
    tool_model_breakdown.insert(
        "mock_ai::unknown".to_string(),
        ToolModelHeadlineStats {
            ai_additions: 4,
            ai_accepted: 3,
        },
    );
    let stats = CommitStats {
        ai_additions: 4,
        ai_accepted: 3,
        tool_model_breakdown,
        ..Default::default()
    };

    assert_eq!(metric_tool_model_breakdown(&stats), None);
}

#[test]
fn test_count_line_ranges_two_ranges() {
    assert_eq!(count_line_ranges(&[1, 2, 3, 10, 11, 12]), 2);
}

#[test]
fn test_should_skip_stats_exactly_at_thresholds() {
    // Exactly at the hunks threshold alone should trigger skip.
    let at_hunks = StatsCostEstimate {
        files_with_additions: 0,
        added_lines: 0,
        hunk_ranges: STATS_SKIP_MAX_HUNKS,
        deleted_lines: 0,
    };
    assert!(
        should_skip_expensive_post_commit_stats(&at_hunks),
        "Exactly at hunk threshold should skip"
    );

    // Exactly at added-lines threshold alone should trigger skip.
    let at_added = StatsCostEstimate {
        files_with_additions: 0,
        added_lines: STATS_SKIP_MAX_ADDED_LINES,
        hunk_ranges: 0,
        deleted_lines: 0,
    };
    assert!(
        should_skip_expensive_post_commit_stats(&at_added),
        "Exactly at added-lines threshold should skip"
    );

    // Exactly at files-with-additions threshold alone should trigger skip.
    let at_files = StatsCostEstimate {
        files_with_additions: STATS_SKIP_MAX_FILES_WITH_ADDITIONS,
        added_lines: 0,
        hunk_ranges: 0,
        deleted_lines: 0,
    };
    assert!(
        should_skip_expensive_post_commit_stats(&at_files),
        "Exactly at files-with-additions threshold should skip"
    );

    // Exactly at deleted-lines threshold alone should trigger skip.
    let at_deleted = StatsCostEstimate {
        files_with_additions: 0,
        added_lines: 0,
        hunk_ranges: 0,
        deleted_lines: STATS_SKIP_MAX_DELETED_LINES,
    };
    assert!(
        should_skip_expensive_post_commit_stats(&at_deleted),
        "Exactly at deleted-lines threshold should skip"
    );

    // All at zero should NOT skip.
    let all_zero = StatsCostEstimate {
        files_with_additions: 0,
        added_lines: 0,
        hunk_ranges: 0,
        deleted_lines: 0,
    };
    assert!(
        !should_skip_expensive_post_commit_stats(&all_zero),
        "All zero values should not skip"
    );
}
