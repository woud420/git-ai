use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};

use chrono::{DateTime, FixedOffset, TimeZone, Utc};

use crate::cli::fail::print_stdout;
use crate::model::authorship_log::PromptRecord;

use super::hunk_parser::BlameRenderPreparation;
use super::{BlameHunk, GitAiBlameOptions};

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

    // The author shown for a line: its AI authorship if any, else git's author.
    let author_display = |line_num: u32, hunk: &BlameHunk| {
        let author = line_authors.get(&line_num).unwrap_or(&hunk.original_author);
        if options.suppress_author {
            String::new()
        } else if options.show_prompt
            && let Some(prompt) = prompt_records.get(author)
        {
            let short_hash = &author[..7.min(author.len())];
            format!("{} [{}]", prompt.agent_id.tool, short_hash)
        } else if options.show_email {
            format!("{} <{}>", author, hunk.author_email)
        } else {
            author.to_string()
        }
    };
    // Pad every author to the widest one displayed. Lines within one hunk can
    // have different authors.
    let max_author_width = prepared
        .requested_lines
        .iter()
        .filter_map(|line_num| {
            let hunk = prepared.line_to_hunk.get(line_num)?;
            Some(author_display(*line_num, hunk).chars().count())
        })
        .max()
        .unwrap_or(0);

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

            // Format date according to options
            let date_str = format_blame_date(hunk.author_time, &hunk.author_tz, options);

            // Pad author name to consistent width
            let padded_author = format!(
                "{:<width$}",
                author_display(line_num, hunk),
                width = max_author_width
            );

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

    // A pager of "cat" or "" means no pager, as in git.
    let paged = pager != "cat"
        && !pager.is_empty()
        && io::stdout().is_terminal()
        && run_pager(&pager, &output);
    if !paged {
        print_stdout(&output);
    }
}

/// Feed `output` to `pager`. Returns false when the pager cannot be started.
fn run_pager(pager: &str, output: &str) -> bool {
    // Like git, run the pager through the shell so `GIT_PAGER="less -R"` works.
    #[cfg(unix)]
    let mut command = {
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(pager);
        command
    };
    #[cfg(not(unix))]
    let mut command = std::process::Command::new(pager);
    let Ok(mut child) = command.stdin(std::process::Stdio::piped()).spawn() else {
        return false;
    };
    // Taking stdin closes it after the write, so a pager reading to EOF ends.
    // A write error means the pager quit early (e.g. `q` in less), as in git.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(output.as_bytes());
    }
    let _ = child.wait();
    true
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn run_pager_passes_arguments_through_the_shell_and_closes_its_input() {
        let dir = tempfile::tempdir().unwrap();
        let paged = dir.path().join("paged");
        let pager = format!("cat > '{}'", paged.display());

        assert!(super::run_pager(&pager, "line 1\nline 2\n"));
        assert_eq!(std::fs::read_to_string(paged).unwrap(), "line 1\nline 2\n");
    }
}
