use super::*;
use insta::assert_debug_snapshot;

#[test]
fn test_terminal_stats_display() {
    // Test with mixed human/AI stats
    let stats = CommitStats {
        human_additions: 50,
        unknown_additions: 0,
        ai_additions: 100,
        ai_accepted: 25,
        git_diff_deleted_lines: 15,
        git_diff_added_lines: 80,
        tool_model_breakdown: BTreeMap::new(),
    };

    let mixed_output = write_stats_to_terminal(&stats, false);
    assert_debug_snapshot!(mixed_output);

    // Test with AI-only stats
    let ai_stats = CommitStats {
        human_additions: 0,
        unknown_additions: 0,
        ai_additions: 100,
        ai_accepted: 95,
        git_diff_deleted_lines: 0,
        git_diff_added_lines: 100,
        tool_model_breakdown: BTreeMap::new(),
    };

    let ai_only_output = write_stats_to_terminal(&ai_stats, false);
    assert_debug_snapshot!(ai_only_output);

    // Test with human-only stats
    let human_stats = CommitStats {
        human_additions: 75,
        unknown_additions: 0,
        ai_additions: 0,
        ai_accepted: 0,
        git_diff_deleted_lines: 10,
        git_diff_added_lines: 75,
        tool_model_breakdown: BTreeMap::new(),
    };

    let human_only_output = write_stats_to_terminal(&human_stats, false);
    assert_debug_snapshot!(human_only_output);

    // Test with minimal human contribution (should get at least 2 blocks)
    let minimal_human_stats = CommitStats {
        human_additions: 2,
        unknown_additions: 0,
        ai_additions: 100,
        ai_accepted: 95,
        git_diff_deleted_lines: 0,
        git_diff_added_lines: 102,
        tool_model_breakdown: BTreeMap::new(),
    };

    let minimal_human_output = write_stats_to_terminal(&minimal_human_stats, false);
    assert_debug_snapshot!(minimal_human_output);

    // Test with deletion-only commit (no additions)
    let deletion_only_stats = CommitStats {
        human_additions: 0,
        unknown_additions: 0,
        ai_additions: 0,
        ai_accepted: 0,
        git_diff_deleted_lines: 25,
        git_diff_added_lines: 0,
        tool_model_breakdown: BTreeMap::new(),
    };

    let deletion_only_output = write_stats_to_terminal(&deletion_only_stats, false);
    assert_debug_snapshot!(deletion_only_output);

    // --- New test cases for untracked segment ---

    // 18% human / 22% untracked / 60% AI — matches the design example
    let untracked_stats = CommitStats {
        human_additions: 180,
        unknown_additions: 220,
        ai_additions: 600,
        ai_accepted: 462,
        git_diff_deleted_lines: 0,
        git_diff_added_lines: 1000,
        tool_model_breakdown: BTreeMap::new(),
    };
    let with_untracked_output = write_stats_to_terminal(&untracked_stats, false);
    assert_debug_snapshot!(with_untracked_output);

    // untracked exactly at the 1% threshold — should NOT show untracked segment
    let threshold_stats = CommitStats {
        human_additions: 49,
        unknown_additions: 1,
        ai_additions: 50,
        ai_accepted: 50,
        git_diff_deleted_lines: 0,
        git_diff_added_lines: 100,
        tool_model_breakdown: BTreeMap::new(),
    };
    let untracked_at_threshold_output = write_stats_to_terminal(&threshold_stats, false);
    assert_debug_snapshot!(untracked_at_threshold_output);

    // untracked just above 1% threshold (~2%) — should show untracked segment
    let above_threshold_stats = CommitStats {
        human_additions: 97,
        unknown_additions: 2,
        ai_additions: 0,
        ai_accepted: 0,
        git_diff_deleted_lines: 0,
        git_diff_added_lines: 99,
        tool_model_breakdown: BTreeMap::new(),
    };
    let untracked_just_above_output = write_stats_to_terminal(&above_threshold_stats, false);
    assert_debug_snapshot!(untracked_just_above_output);

    // 100% untracked — entire bar is · chars
    let all_untracked_stats = CommitStats {
        human_additions: 0,
        unknown_additions: 100,
        ai_additions: 0,
        ai_accepted: 0,
        git_diff_deleted_lines: 0,
        git_diff_added_lines: 100,
        tool_model_breakdown: BTreeMap::new(),
    };
    let all_untracked_output = write_stats_to_terminal(&all_untracked_stats, false);
    assert_debug_snapshot!(all_untracked_output);

    // OSC 8 hyperlink emitted when is_interactive = true
    // Not a snapshot test — asserts presence of the escape sequence directly.
    let hyperlink_output = write_stats_to_terminal(&untracked_stats, true);
    assert!(
        hyperlink_output.contains("\x1b]8;;https://usegitai.com/docs/cli/untracked\x1b\\"),
        "Expected OSC 8 hyperlink in interactive output, got: {:?}",
        hyperlink_output
    );
    assert!(
        hyperlink_output.contains("untracked"),
        "Expected 'untracked' label in interactive output"
    );
}

#[test]
fn test_markdown_stats_display() {
    // Test with mixed human/AI stats
    let stats = CommitStats {
        human_additions: 50,
        unknown_additions: 0,
        ai_additions: 100,
        ai_accepted: 25,
        git_diff_deleted_lines: 15,
        git_diff_added_lines: 80,
        tool_model_breakdown: BTreeMap::new(),
    };

    let mixed_output = write_stats_to_markdown(&stats);
    assert_debug_snapshot!(mixed_output);

    // Test with AI-only stats
    let ai_stats = CommitStats {
        human_additions: 0,
        unknown_additions: 0,
        ai_additions: 100,
        ai_accepted: 95,
        git_diff_deleted_lines: 0,
        git_diff_added_lines: 100,
        tool_model_breakdown: BTreeMap::new(),
    };

    let ai_only_output = write_stats_to_markdown(&ai_stats);
    assert_debug_snapshot!(ai_only_output);

    // Test with human-only stats
    let human_stats = CommitStats {
        human_additions: 75,
        unknown_additions: 0,
        ai_additions: 0,
        ai_accepted: 0,
        git_diff_deleted_lines: 10,
        git_diff_added_lines: 75,
        tool_model_breakdown: BTreeMap::new(),
    };

    let human_only_output = write_stats_to_markdown(&human_stats);
    assert_debug_snapshot!(human_only_output);

    // Test with minimal human contribution (should get at least 2 blocks)
    let minimal_human_stats = CommitStats {
        human_additions: 2,
        unknown_additions: 0,
        ai_additions: 100,
        ai_accepted: 95,
        git_diff_deleted_lines: 0,
        git_diff_added_lines: 102,
        tool_model_breakdown: BTreeMap::new(),
    };

    let minimal_human_output = write_stats_to_markdown(&minimal_human_stats);
    assert_debug_snapshot!(minimal_human_output);

    // Test with deletion-only commit (no additions)
    let deletion_only_stats = CommitStats {
        human_additions: 0,
        unknown_additions: 0,
        ai_additions: 0,
        ai_accepted: 0,
        git_diff_deleted_lines: 25,
        git_diff_added_lines: 0,
        tool_model_breakdown: BTreeMap::new(),
    };

    let deletion_only_output = write_stats_to_markdown(&deletion_only_stats);
    assert_debug_snapshot!(deletion_only_output);
}
