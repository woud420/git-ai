use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};

use chrono::{DateTime, FixedOffset, TimeZone, Utc};

use crate::model::authorship_log::PromptRecord;

use super::GitAiBlameOptions;
use super::hunk_parser::BlameRenderPreparation;

pub(super) fn format_blame_date(
    author_time: i64,
    author_tz: &str,
    options: &GitAiBlameOptions,
) -> String {
    let dt = DateTime::from_timestamp(author_time, 0)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());

    // Parse timezone string like +0200 or -0500
    let offset = if author_tz.len() == 5 {
        let sign = if &author_tz[0..1] == "+" { 1 } else { -1 };
        let hours: i32 = author_tz[1..3].parse().unwrap_or(0);
        let mins: i32 = author_tz[3..5].parse().unwrap_or(0);
        FixedOffset::east_opt(sign * (hours * 3600 + mins * 60))
            .unwrap_or(FixedOffset::east_opt(0).unwrap())
    } else {
        FixedOffset::east_opt(0).unwrap()
    };

    let dt = offset.from_utc_datetime(&dt.naive_utc());

    // Format date according to options (default: iso)
    if let Some(fmt) = &options.date_format {
        // TODO: support all git date formats
        match fmt.as_str() {
            "iso" | "iso8601" => dt.format("%Y-%m-%d %H:%M:%S %z").to_string(),
            "short" => dt.format("%Y-%m-%d").to_string(),
            "relative" => format!("{} seconds ago", (Utc::now().timestamp() - author_time)),
            _ => dt.format("%Y-%m-%d %H:%M:%S %z").to_string(),
        }
    } else {
        dt.format("%Y-%m-%d %H:%M:%S %z").to_string()
    }
}

pub(super) fn output_default_format(
    prepared: &BlameRenderPreparation,
    line_authors: &HashMap<u32, String>,
    prompt_records: &HashMap<String, PromptRecord>,
    file_path: &str,
    lines: &[&str],
    options: &GitAiBlameOptions,
) {
    let mut output = String::new();

    // Calculate the maximum line number width for proper padding
    let max_line_num = lines.len() as u32;
    let line_num_width = max_line_num.to_string().len();

    // Calculate the maximum author name width for proper padding
    let mut max_author_width = 0;
    for hunk in &prepared.hunks {
        let author = line_authors
            .get(&hunk.range.0)
            .unwrap_or(&hunk.original_author);
        let author_display = if options.suppress_author {
            "".to_string()
        } else if options.show_prompt && prompt_records.contains_key(author) {
            let prompt = &prompt_records[author];
            let short_hash = &author[..7.min(author.len())];
            format!("{} [{}]", prompt.agent_id.tool, short_hash)
        } else if options.show_email {
            format!("{} <{}>", author, hunk.author_email)
        } else {
            author.to_string()
        };
        max_author_width = max_author_width.max(author_display.len());
    }

    let hash_column_width = if options.long_rev {
        40
    } else {
        prepared
            .hunks
            .iter()
            .map(|hunk| hunk.abbrev_sha.len() + Repository::blame_hunk_marker_width(hunk, options))
            .max()
            .unwrap_or(0)
    };

    for &line_num in &prepared.requested_lines {
        let line_index = (line_num - 1) as usize;
        let line_content = if line_index < lines.len() {
            lines[line_index]
        } else {
            ""
        };

        if let Some(hunk) = prepared.line_to_hunk.get(&line_num) {
            let sha = &hunk.abbrev_sha;

            // Markers share the hash column, including -l and blank boundaries.
            let mut full_sha = String::new();
            if hunk.is_boundary && !options.blank_boundary && !options.show_root {
                full_sha.push('^');
            }
            if hunk.is_unblamable {
                full_sha.push('*');
            }
            if hunk.is_ignored {
                full_sha.push('?');
            }
            let remaining_width = hash_column_width.saturating_sub(full_sha.len());
            if hunk.is_boundary && options.blank_boundary && !options.show_root {
                full_sha.push_str(&" ".repeat(remaining_width.min(sha.len())));
            } else if options.long_rev {
                full_sha.push_str(&sha[..sha.len().min(remaining_width)]);
            } else {
                full_sha.push_str(sha);
            }

            // Get the author for this line (AI authorship or original)
            let author = line_authors.get(&line_num).unwrap_or(&hunk.original_author);

            // Format date according to options
            let date_str = format_blame_date(hunk.author_time, &hunk.author_tz, options);

            // Handle different output formats based on flags
            let author_display = if options.suppress_author {
                "".to_string()
            } else if options.show_prompt && prompt_records.contains_key(author) {
                let prompt = &prompt_records[author];
                let short_hash = &author[..7.min(author.len())];
                format!("{} [{}]", prompt.agent_id.tool, short_hash)
            } else if options.show_email {
                format!("{} <{}>", author, hunk.author_email)
            } else {
                author.to_string()
            };

            // Pad author name to consistent width
            let padded_author = if max_author_width > 0 {
                format!("{:<width$}", author_display, width = max_author_width)
            } else {
                author_display
            };

            // Format exactly like git blame: sha (author date line) code
            if options.suppress_author {
                // Suppress author format: sha line_number) code
                output.push_str(&format!("{} {}) {}\n", full_sha, line_num, line_content));
            } else {
                // Normal format: sha (author date line) code
                if options.show_name {
                    // Show filename format: sha filename (author date line) code
                    output.push_str(&format!(
                        "{} {} ({} {} {:>width$}) {}\n",
                        full_sha,
                        file_path,
                        padded_author,
                        date_str,
                        line_num,
                        line_content,
                        width = line_num_width
                    ));
                } else if options.show_number {
                    // Show number format: sha line_number (author date line) code (matches git's -n output)
                    output.push_str(&format!(
                        "{} {} ({} {} {:>width$}) {}\n",
                        full_sha,
                        line_num,
                        padded_author,
                        date_str,
                        line_num,
                        line_content,
                        width = line_num_width
                    ));
                } else {
                    // Normal format: sha (author date line) code
                    output.push_str(&format!(
                        "{} ({} {} {:>width$}) {}\n",
                        full_sha,
                        padded_author,
                        date_str,
                        line_num,
                        line_content,
                        width = line_num_width
                    ));
                }
            }
        } else {
            // Fallback for lines without blame info
            output.push_str(&format!(
                "{:<8} (unknown        1970-01-01 00:00:00 +0000    {:>width$}) {}\n",
                "????????",
                line_num,
                line_content,
                width = line_num_width
            ));
        }
    }

    // Print stats if requested (at the end, like git blame)
    if options.show_stats {
        // Append git-like stats lines to output string
        let stats = "num read blob: 1\nnum get patch: 0\nnum commits: 0\n";
        output.push_str(stats);
    }

    // Output handling - respect pager environment variables
    let pager = std::env::var("GIT_PAGER")
        .or_else(|_| std::env::var("PAGER"))
        .unwrap_or_else(|_| "less".to_string());

    // If pager is set to "cat" or empty, output directly
    if pager == "cat" || pager.is_empty() {
        print!("{}", output);
    } else if io::stdout().is_terminal() {
        // Try to use the specified pager
        match std::process::Command::new(&pager)
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.as_mut() {
                    if stdin.write_all(output.as_bytes()).is_ok() {
                        let _ = child.wait();
                    } else {
                        // Fall back to direct output if pager fails
                        print!("{}", output);
                    }
                } else {
                    // Fall back to direct output if pager fails
                    print!("{}", output);
                }
            }
            Err(_) => {
                // Fall back to direct output if pager fails
                print!("{}", output);
            }
        }
    } else {
        // Not a terminal, output directly
        print!("{}", output);
    }
}
