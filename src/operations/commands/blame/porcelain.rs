use std::collections::HashSet;
use std::fmt::Write as _;

use crate::cli::fail::print_stdout;
use crate::operations::authorship::agent_detection::match_email_to_agent;

use super::hunk_parser::BlameRenderPreparation;
use super::{BlameHunk, GitAiBlameOptions};

/// Author shown for a hunk. Commits without an authorship note whose author
/// email belongs to a known agent show the tool name; all others show git's
/// original author.
fn display_author<'a>(hunk: &'a BlameHunk, commits_with_notes: &HashSet<String>) -> &'a str {
    if commits_with_notes.contains(&hunk.commit_sha) {
        return &hunk.original_author;
    }
    match_email_to_agent(&hunk.author_email).unwrap_or(&hunk.original_author)
}

/// `<sha> <orig line> <final line>`, plus the group size on a hunk's first line.
fn write_header(out: &mut String, hunk: &BlameHunk, line_num: u32, first_in_hunk: bool) {
    let orig_line = hunk.orig_range.0 + (line_num - hunk.range.0);
    if first_in_hunk {
        let group_size = hunk.range.1 - hunk.range.0 + 1;
        let _ = writeln!(
            out,
            "{} {orig_line} {line_num} {group_size}",
            hunk.commit_sha
        );
    } else {
        let _ = writeln!(out, "{} {orig_line} {line_num}", hunk.commit_sha);
    }
}

/// The commit block git prints between a header and the `filename` line.
fn write_commit_metadata(out: &mut String, hunk: &BlameHunk, commits_with_notes: &HashSet<String>) {
    let _ = writeln!(out, "author {}", display_author(hunk, commits_with_notes));
    let _ = writeln!(out, "author-mail <{}>", hunk.author_email);
    let _ = writeln!(out, "author-time {}", hunk.author_time);
    let _ = writeln!(out, "author-tz {}", hunk.author_tz);
    let _ = writeln!(out, "committer {}", hunk.committer);
    let _ = writeln!(out, "committer-mail <{}>", hunk.committer_email);
    let _ = writeln!(out, "committer-time {}", hunk.committer_time);
    let _ = writeln!(out, "committer-tz {}", hunk.committer_tz);
    let _ = writeln!(out, "summary {}", hunk.summary);
    if hunk.is_boundary {
        let _ = writeln!(out, "boundary");
    }
    if let Some(previous) = &hunk.previous {
        let _ = writeln!(out, "previous {previous}");
    }
}

pub(super) fn output_porcelain_format(
    prepared: &BlameRenderPreparation,
    lines: &[&str],
    options: &GitAiBlameOptions,
    commits_with_notes: &HashSet<String>,
) {
    let mut out = String::new();
    let mut last_hunk = None;
    let mut seen_commits: HashSet<&str> = HashSet::new();
    for &line_num in &prepared.requested_lines {
        let Some(hunk) = prepared.line_to_hunk.get(&line_num) else {
            continue;
        };
        let hunk_id = (hunk.commit_sha.as_str(), hunk.range.0);
        let first_in_hunk = last_hunk != Some(hunk_id);
        last_hunk = Some(hunk_id);

        write_header(&mut out, hunk, line_num, first_in_hunk);
        // --line-porcelain repeats the commit block on every line; --porcelain
        // prints it once, with the first hunk of each commit.
        if options.line_porcelain || (first_in_hunk && seen_commits.insert(&hunk.commit_sha)) {
            write_commit_metadata(&mut out, hunk, commits_with_notes);
            let _ = writeln!(out, "filename {}", hunk.git_filename);
        }
        let _ = writeln!(
            out,
            "\t{}",
            lines.get((line_num - 1) as usize).unwrap_or(&"")
        );
    }
    print_stdout(&out);
}

/// One entry per hunk, and no content lines.
pub(super) fn output_incremental_format(
    prepared: &BlameRenderPreparation,
    commits_with_notes: &HashSet<String>,
) {
    let mut out = String::new();
    let mut last_hunk = None;
    let mut seen_commits: HashSet<&str> = HashSet::new();
    for &line_num in &prepared.requested_lines {
        let Some(hunk) = prepared.line_to_hunk.get(&line_num) else {
            continue;
        };
        let hunk_id = (hunk.commit_sha.as_str(), hunk.range.0);
        if last_hunk == Some(hunk_id) {
            continue;
        }
        last_hunk = Some(hunk_id);

        write_header(&mut out, hunk, line_num, true);
        if seen_commits.insert(&hunk.commit_sha) {
            write_commit_metadata(&mut out, hunk, commits_with_notes);
        }
        let _ = writeln!(out, "filename {}", hunk.git_filename);
    }
    print_stdout(&out);
}
