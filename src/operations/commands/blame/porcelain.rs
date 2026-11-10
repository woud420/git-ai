use std::collections::HashSet;

use crate::error::GitAiError;
use crate::operations::authorship::agent_detection::match_email_to_agent;
use crate::operations::git::repository::Repository;

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
fn print_header(hunk: &BlameHunk, line_num: u32, first_in_hunk: bool) {
    let orig_line = hunk.orig_range.0 + (line_num - hunk.range.0);
    if first_in_hunk {
        let group_size = hunk.range.1 - hunk.range.0 + 1;
        println!("{} {orig_line} {line_num} {group_size}", hunk.commit_sha);
    } else {
        println!("{} {orig_line} {line_num}", hunk.commit_sha);
    }
}

/// The commit block git prints between a header and the `filename` line.
fn print_commit_metadata(hunk: &BlameHunk, commits_with_notes: &HashSet<String>) {
    println!("author {}", display_author(hunk, commits_with_notes));
    println!("author-mail <{}>", hunk.author_email);
    println!("author-time {}", hunk.author_time);
    println!("author-tz {}", hunk.author_tz);
    println!("committer {}", hunk.committer);
    println!("committer-mail <{}>", hunk.committer_email);
    println!("committer-time {}", hunk.committer_time);
    println!("committer-tz {}", hunk.committer_tz);
    println!("summary {}", hunk.summary);
    if hunk.is_boundary {
        println!("boundary");
    }
    if let Some(previous) = &hunk.previous {
        println!("previous {previous}");
    }
}

pub(super) fn output_porcelain_format(
    repo: &Repository,
    file_path: &str,
    lines: &[&str],
    line_ranges: &[(u32, u32)],
    options: &GitAiBlameOptions,
    commits_with_notes: &HashSet<String>,
) -> Result<(), GitAiError> {
    let prepared = repo.prepare_blame_render(file_path, line_ranges, options)?;

    let mut seen_commits: HashSet<&str> = HashSet::new();
    for line_num in prepared.requested_lines {
        let Some(hunk) = prepared.line_to_hunk.get(&line_num) else {
            continue;
        };
        let first_in_hunk = line_num == hunk.range.0;
        print_header(hunk, line_num, first_in_hunk);
        // --line-porcelain repeats the commit block on every line; --porcelain
        // prints it once, with the first hunk of each commit.
        if options.line_porcelain || (first_in_hunk && seen_commits.insert(&hunk.commit_sha)) {
            print_commit_metadata(hunk, commits_with_notes);
            println!("filename {}", hunk.git_filename);
        }
        println!("\t{}", lines.get((line_num - 1) as usize).unwrap_or(&""));
    }
    Ok(())
}

pub(super) fn output_incremental_format(
    repo: &Repository,
    file_path: &str,
    line_ranges: &[(u32, u32)],
    options: &GitAiBlameOptions,
    commits_with_notes: &HashSet<String>,
) -> Result<(), GitAiError> {
    let prepared = repo.prepare_blame_render(file_path, line_ranges, options)?;

    // One entry per hunk, and no content lines.
    let mut seen_commits: HashSet<&str> = HashSet::new();
    for line_num in prepared.requested_lines {
        let Some(hunk) = prepared.line_to_hunk.get(&line_num) else {
            continue;
        };
        if line_num != hunk.range.0 {
            continue;
        }
        print_header(hunk, line_num, true);
        if seen_commits.insert(&hunk.commit_sha) {
            print_commit_metadata(hunk, commits_with_notes);
        }
        println!("filename {}", hunk.git_filename);
    }
    Ok(())
}
