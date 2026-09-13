use super::CommitStats;

pub fn write_stats_to_terminal(stats: &CommitStats, is_interactive: bool) -> String {
    let mut output = String::new();

    // Set maximum bar width to 40 characters
    let bar_width: usize = 40;

    // Handle deletion-only commits (no additions)
    if stats.git_diff_added_lines == 0 && stats.git_diff_deleted_lines > 0 {
        // Show gray bar for deletion-only commit
        let mut progress_bar = String::new();
        progress_bar.push_str("you  ");
        progress_bar.push_str("\x1b[90m"); // Gray color
        progress_bar.push_str(&" ".repeat(bar_width)); // Gray bar
        progress_bar.push_str("\x1b[0m"); // Reset color
        progress_bar.push_str(" ai");

        output.push_str(&progress_bar);
        output.push('\n');
        if is_interactive {
            println!("{}", progress_bar);
        }

        // Show "(no additions)" message below the bar
        let no_additions_msg = format!("     \x1b[90m{:^40}\x1b[0m", "(no additions)");
        output.push_str(&no_additions_msg);
        output.push('\n');
        if is_interactive {
            println!("{}", no_additions_msg);
        }
        // No percentage line or AI stats for deletion-only commits
        return output;
    }

    // Calculate total additions: known human + unknown (untracked) + AI
    let total_additions = stats.human_additions + stats.unknown_additions + stats.ai_additions;

    // (ai_additions == ai_accepted after mixed removal, so acceptance is always 100%)

    // Determine whether to show the untracked segment (raw float check, before rounding)
    let untracked_pct_raw = if total_additions > 0 {
        stats.unknown_additions as f64 / total_additions as f64 * 100.0
    } else {
        0.0
    };
    let show_untracked = untracked_pct_raw > 1.0;

    // Calculate human bar segment
    let human_bars = if total_additions > 0 {
        ((stats.human_additions as f64 / total_additions as f64) * bar_width as f64) as usize
    } else {
        0
    };

    // Ensure human contributions get at least 2 visible blocks if they have more than 1 line
    let min_human_bars = if stats.human_additions > 1 { 2 } else { 0 };
    let final_human_bars = human_bars.max(min_human_bars);

    // Distribute remaining width between untracked and AI proportionally.
    // When untracked is below the 1% threshold, all remaining width goes to AI.
    let remaining_width = bar_width.saturating_sub(final_human_bars);
    let (final_untracked_bars, final_ai_bars) = if show_untracked {
        let total_other = stats.unknown_additions + stats.ai_additions;
        let untracked_bars = if total_other > 0 {
            ((stats.unknown_additions as f64 / total_other as f64) * remaining_width as f64)
                as usize
        } else {
            0
        };
        (
            untracked_bars,
            remaining_width.saturating_sub(untracked_bars),
        )
    } else {
        (0, remaining_width)
    };

    // Build the progress bar
    let mut progress_bar = String::new();
    progress_bar.push_str("you  ");
    progress_bar.push_str(&"█".repeat(final_human_bars)); // known human (attested)
    progress_bar.push_str(&"·".repeat(final_untracked_bars)); // untracked (no attestation)
    progress_bar.push_str(&"░".repeat(final_ai_bars)); // AI
    progress_bar.push_str(" ai");

    // Calculate percentages for display
    let human_percentage = if total_additions > 0 {
        ((stats.human_additions as f64 / total_additions as f64) * 100.0).round() as u32
    } else {
        0
    };
    let ai_percentage = if total_additions > 0 {
        ((stats.ai_additions as f64 / total_additions as f64) * 100.0).round() as u32
    } else {
        0
    };

    // Print the stats
    output.push_str(&progress_bar);
    output.push('\n');
    if is_interactive {
        println!("{}", progress_bar);
    }

    // Percentage line: three anchors (human / untracked / AI) when untracked is visible,
    // two anchors (human / AI) otherwise.
    if show_untracked {
        let untracked_percentage = untracked_pct_raw.round() as u32;
        // When interactive, wrap "untracked" in an OSC 8 hyperlink so it is clickable in
        // supporting terminals (iTerm2, Warp, etc.). Spaces are constructed manually —
        // not via format-width padding on the label — so that invisible escape bytes do
        // not misalign the output.
        let untracked_label = if is_interactive {
            "\x1b]8;;https://usegitai.com/docs/cli/untracked\x1b\\\x1b[4muntracked\x1b[24m\x1b]8;;\x1b\\"
                .to_string()
        } else {
            "untracked".to_string()
        };
        let percentage_line = format!(
            "     {:<3}{:>10}{} {:>3}%{:>10}{:>3}%",
            format!("{}%", human_percentage),
            "",
            untracked_label,
            untracked_percentage,
            "",
            ai_percentage
        );
        output.push_str(&percentage_line);
        output.push('\n');
        if is_interactive {
            println!("{}", percentage_line);
        }
    } else {
        let percentage_line = format!(
            "     {:<3}{:>33}{:>3}%",
            format!("{}%", human_percentage),
            "",
            ai_percentage
        );
        output.push_str(&percentage_line);
        output.push('\n');
        if is_interactive {
            println!("{}", percentage_line);
        }
    }

    output
}

#[allow(dead_code)]
pub fn write_stats_to_markdown(stats: &CommitStats) -> String {
    let mut output = String::new();

    // Set maximum bar width to 20 characters
    let bar_width: usize = 20;

    // Handle deletion-only commits (no additions)
    if stats.git_diff_added_lines == 0 && stats.git_diff_deleted_lines > 0 {
        output.push_str("(no additions)");
        output.push('\n');
        return output;
    }

    // Calculate total additions for the progress bar
    let total_additions = stats.git_diff_added_lines;

    // Human additions: known-human attested + unattested
    let pure_human = stats.human_additions + stats.unknown_additions;
    // AI = AI lines accepted
    let pure_ai = stats.ai_accepted;

    // Calculate percentages for display
    let pure_human_percentage = if total_additions > 0 {
        ((pure_human as f64 / total_additions as f64) * 100.0).round() as u32
    } else {
        0
    };
    let ai_percentage = if total_additions > 0 {
        ((pure_ai as f64 / total_additions as f64) * 100.0).round() as u32
    } else {
        0
    };

    // Calculate bar sizes
    let pure_human_bars = if total_additions > 0 {
        let calculated =
            ((pure_human as f64 / total_additions as f64) * bar_width as f64).round() as usize;
        // Ensure at least 1 block if value > 0
        if pure_human > 0 && calculated == 0 {
            1
        } else {
            calculated
        }
    } else {
        0
    };

    let ai_bars = if total_additions > 0 {
        let calculated =
            ((pure_ai as f64 / total_additions as f64) * bar_width as f64).round() as usize;
        // Ensure at least 1 block if value > 0
        if pure_ai > 0 && calculated == 0 {
            1
        } else {
            calculated
        }
    } else {
        0
    };

    output.push_str("Stats powered by [Git AI](https://github.com/git-ai-project/git-ai)\n\n");
    // Build the fenced code block
    output.push_str("```text\n");

    // Human line: dark blocks for human, light blocks for rest
    output.push_str("🧠 you    ");
    output.push_str(&"█".repeat(pure_human_bars));
    output.push_str(&"░".repeat(bar_width.saturating_sub(pure_human_bars)));
    output.push_str(&format!("  {}%\n", pure_human_percentage));

    // AI line: light blocks for non-ai, dark blocks for ai
    output.push_str("🤖 ai     ");
    output.push_str(&"░".repeat(bar_width.saturating_sub(ai_bars)));
    output.push_str(&"█".repeat(ai_bars));
    output.push_str(&format!("  {}%\n", ai_percentage));

    output.push_str("```");

    // Add details section
    output.push_str("\n\n<details>\n");
    output.push_str("<summary>More stats</summary>\n\n");

    // Find top model by accepted lines
    if !stats.tool_model_breakdown.is_empty()
        && let Some((model_name, model_stats)) = stats
            .tool_model_breakdown
            .iter()
            .max_by_key(|(_, stats)| stats.ai_accepted)
    {
        output.push_str(&format!(
            "- Top model: {} ({} accepted lines)\n",
            model_name, model_stats.ai_accepted
        ));
    }

    output.push_str("\n</details>");

    output
}
