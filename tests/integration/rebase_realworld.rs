//! Comprehensive real-world rebase attribution tests.
//!
//! These tests cover four rebase scenario categories with ≥5 commits per branch,
//! verifying line-level attribution at EVERY rebased commit — not just HEAD.
//! Tests are intentionally strict: they surface bugs in the slow-path attribution
//! rewriting code (src/authorship/rewrite.rs).
//!
//! IMPORTANT: All attribution reads MUST go through TestRepo helpers:
//!   - `run_blame_api(repo, sha, file, ctx)` — blame at specific commit via Rust API (newest_commit)
//!   - `repo.read_authorship_note(sha)` — waits for daemon sync
//!
//! Never call git/git-ai directly (racy in daemon mode).
//!
//! Four scenario categories (10 tests each):
//!   1. Fast path  — disjoint file sets between branches
//!   2. Slow path  — same files modified non-conflictingly (upstream prepends)
//!   3. Human conflict — conflict resolved by human (fs::write, no checkpoint)
//!   4. AI conflict    — conflict resolved by AI (set_contents with .ai())

#![allow(dead_code)]
use crate::test_utils::session_line_count;
use std::fs;

use crate::repos::blame_support::is_ai_blame_author;
use crate::repos::test_file::{ExpectedLine, ExpectedLineExt};
use crate::repos::test_repo::TestRepo;
use git_ai::model::authorship_log_serialization::AuthorshipLog;
use git_ai::operations::commands::blame::GitAiBlameOptions;
use git_ai::operations::git::repository as GitAiRepository;

// ============================================================================
// Shared helpers — ALL note/blame reads go through TestRepo helpers
// ============================================================================

/// Parse the authorship note for `sha`.  Panics if the note is absent.
/// Uses the shared TestRepo helper for daemon-safe access and deserialization.
fn parse_note(repo: &TestRepo, sha: &str) -> AuthorshipLog {
    repo.require_authorship_log(sha)
}

/// Return the N most-recent commit SHAs ordered oldest→newest:
/// [HEAD~(n-1), HEAD~(n-2), …, HEAD~1, HEAD].
fn get_commit_chain(repo: &TestRepo, n: usize) -> Vec<String> {
    (0..n)
        .rev()
        .map(|offset| {
            let rev = if offset == 0 {
                "HEAD".to_string()
            } else {
                format!("HEAD~{}", offset)
            };
            repo.git(&["rev-parse", &rev]).unwrap().trim().to_string()
        })
        .collect()
}

/// Sum of `accepted_lines` across all prompts in a note string.
fn total_accepted_lines(note: &str) -> u32 {
    let log = AuthorshipLog::deserialize_from_string(note).expect("should parse authorship note");
    // Session format: count AI-attested lines from attestation entries.
    // Old format: fall back to prompts accepted_lines.
    let session_lines = session_line_count(&log);
    if session_lines > 0 {
        return session_lines;
    }
    log.metadata
        .prompts
        .values()
        .map(|p| p.accepted_lines)
        .sum()
}

/// File paths listed in a note's attestations section.
fn files_in_note(note: &str) -> Vec<String> {
    let log = AuthorshipLog::deserialize_from_string(note).expect("should parse authorship note");
    log.attestations
        .iter()
        .map(|a| a.file_path.clone())
        .collect()
}

/// Assert that `sha`'s note lists EXACTLY the files in `expected` — no extras,
/// no missing entries.  Uses substring matching for paths (e.g. "users.py"
/// matches "src/users.py").
fn assert_note_files_exact(repo: &TestRepo, sha: &str, ctx: &str, expected: &[&str]) {
    let raw = repo
        .read_authorship_note(sha)
        .unwrap_or_else(|| panic!("{}: commit {} has no note", ctx, sha));
    let actual = files_in_note(&raw);
    // Every actual file must be in expected
    for f in &actual {
        assert!(
            expected.iter().any(|e| f.contains(e)),
            "{}: unexpected file '{}' in note for {}.\nExpected only: {:?}\nGot: {:?}",
            ctx,
            f,
            sha,
            expected,
            actual
        );
    }
    // Every expected file must appear in actual
    for e in expected {
        assert!(
            actual.iter().any(|f| f.contains(e)),
            "{}: expected file '{}' missing from note for {}.\nExpected: {:?}\nGot: {:?}",
            ctx,
            e,
            sha,
            expected,
            actual
        );
    }
}

/// Assert none of `forbidden` appear in `sha`'s note.
fn assert_note_no_forbidden_files(repo: &TestRepo, sha: &str, ctx: &str, forbidden: &[&str]) {
    let raw = repo
        .read_authorship_note(sha)
        .unwrap_or_else(|| panic!("{}: commit {} has no note", ctx, sha));
    let actual = files_in_note(&raw);
    for f in forbidden {
        assert!(
            !actual.iter().any(|a| a.contains(f)),
            "{}: forbidden file '{}' appears in note for {}.\nAll files: {:?}",
            ctx,
            f,
            sha,
            actual
        );
    }
}

/// Like `assert_note_no_forbidden_files` but silently passes when the commit has no note.
/// Use for human commits (created via `commit_untracked_file`) that correctly produce no note
/// after rebase — if a note does exist (e.g. implementation creates empty propagation
/// notes), the forbidden-file check is still enforced.
fn assert_note_no_forbidden_files_if_present(
    repo: &TestRepo,
    sha: &str,
    ctx: &str,
    forbidden: &[&str],
) {
    let Some(raw) = repo.read_authorship_note(sha) else {
        return; // no note — human commit, trivially correct
    };
    let actual = files_in_note(&raw);
    for f in forbidden {
        assert!(
            !actual.iter().any(|a| a.contains(f)),
            "{}: forbidden file '{}' appears in note for {}.\nAll files: {:?}",
            ctx,
            f,
            sha,
            actual
        );
    }
}

/// Verify that specific lines (identified by content substring) carry the expected
/// AI-or-human attribution for `file` at `sha`.
/// This is a *sample* check — caller need not list every line.
/// Uses the Rust blame API with `newest_commit` set for accurate per-commit attribution.
fn assert_blame_sample_at_commit(
    repo: &TestRepo,
    sha: &str,
    file: &str,
    ctx: &str,
    samples: &[(&str, bool)],
) {
    let (line_authors, lines) = run_blame_api(repo, sha, file, ctx);
    for (exp_substr, exp_is_ai) in samples {
        let found = lines
            .iter()
            .enumerate()
            .find(|(_, l)| l.contains(exp_substr));
        let (idx, line_text) = found.unwrap_or_else(|| {
            panic!(
                "{}: line containing {:?} not found in {} at {}\nFile lines:\n{}",
                ctx,
                exp_substr,
                file,
                sha,
                lines.join("\n")
            )
        });
        let line_num = (idx + 1) as u32;
        let author = line_authors
            .get(&line_num)
            .map(|s| s.as_str())
            .unwrap_or("Test User");
        let got_ai = is_ai_blame_author(author);
        assert_eq!(
            got_ai,
            *exp_is_ai,
            "{}: line {} ({:?}) expected {}AI-authored but got author={:?}\nat {} file {}",
            ctx,
            line_num,
            line_text,
            if *exp_is_ai { "" } else { "non-" },
            author,
            sha,
            file
        );
    }
}

/// Assert `base_commit_sha` in `sha`'s note equals `sha` itself.
fn assert_note_base_commit_matches(repo: &TestRepo, sha: &str, ctx: &str) {
    let log = parse_note(repo, sha);
    assert_eq!(
        log.metadata.base_commit_sha, sha,
        "{}: base_commit_sha mismatch at {}",
        ctx, sha
    );
}

/// Assert total accepted_lines in `sha`'s note equals `expected` exactly.
fn assert_accepted_lines_exact(repo: &TestRepo, sha: &str, ctx: &str, expected: u32) {
    let raw = repo
        .read_authorship_note(sha)
        .unwrap_or_else(|| panic!("{}: commit {} has no note", ctx, sha));
    let actual = total_accepted_lines(&raw);
    assert_eq!(
        actual, expected,
        "{}: accepted_lines at {} = {} but expected exactly {}",
        ctx, sha, actual, expected
    );
}

/// Assert accepted_lines values are strictly monotonically non-decreasing
/// along the chain (oldest→newest).  Panics on any violation.
fn assert_accepted_lines_monotonic(repo: &TestRepo, ctx: &str, chain: &[String]) {
    let values: Vec<u32> = chain
        .iter()
        .map(|sha| {
            let raw = repo
                .read_authorship_note(sha)
                .unwrap_or_else(|| panic!("{}: commit {} has no note", ctx, sha));
            total_accepted_lines(&raw)
        })
        .collect();
    for i in 1..values.len() {
        assert!(
            values[i] >= values[i - 1],
            "{}: accepted_lines not monotonic: chain[{}]={} > chain[{}]={}\nFull chain values: {:?}",
            ctx,
            i - 1,
            values[i - 1],
            i,
            values[i],
            values
        );
    }
}

/// Assert line-level blame at a specific commit SHA.
/// `expected`: ordered list of (content_substring, is_ai) for every line.
/// Uses the Rust blame API with `newest_commit` set — content from `git show` and
/// attribution from blame come from the same commit, so line counts always agree.
fn assert_blame_at_commit(
    repo: &TestRepo,
    sha: &str,
    file: &str,
    ctx: &str,
    expected: &[(&str, bool)],
) {
    let (line_authors, lines) = run_blame_api(repo, sha, file, ctx);

    assert_eq!(
        lines.len(),
        expected.len(),
        "{}: file {} at {} has {} lines, expected {}\nLines:\n{}",
        ctx,
        file,
        sha,
        lines.len(),
        expected.len(),
        lines.join("\n")
    );

    for (i, (line_text, (exp_substr, exp_is_ai))) in lines.iter().zip(expected.iter()).enumerate() {
        let line_num = (i + 1) as u32;
        assert!(
            line_text.contains(exp_substr),
            "{}: line {} {:?} does not contain {:?}\nat {} file {}",
            ctx,
            line_num,
            line_text,
            exp_substr,
            sha,
            file
        );
        let author = line_authors
            .get(&line_num)
            .map(|s| s.as_str())
            .unwrap_or("Test User");
        let got_ai = is_ai_blame_author(author);
        assert_eq!(
            got_ai,
            *exp_is_ai,
            "{}: line {} ({:?}) expected {}AI-authored but got author={:?}\nat {} file {}",
            ctx,
            line_num,
            line_text,
            if *exp_is_ai { "" } else { "non-" },
            author,
            sha,
            file
        );
    }
}

/// Run the blame Rust API at `sha` for `file`.
/// Returns (line_authors map, file lines).
/// Line splitting mirrors how git counts lines: trailing `\n` does NOT create
/// a phantom empty last line, but a real blank line (double `\n\n`) does.
fn run_blame_api(
    repo: &TestRepo,
    sha: &str,
    file: &str,
    ctx: &str,
) -> (std::collections::HashMap<u32, String>, Vec<String>) {
    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .unwrap_or_else(|e| panic!("{}: find_repository_in_path failed: {}", ctx, e));
    let options = GitAiBlameOptions {
        newest_commit: Some(sha.to_string()),
        no_output: true,
        ..Default::default()
    };
    let (line_authors, _) = gitai_repo
        .blame(file, &options)
        .unwrap_or_else(|e| panic!("{}: blame({}, {}) failed: {}", ctx, sha, file, e));

    // Get file content at the commit for line content verification.
    // Split with split('\n') and remove the single trailing empty element produced
    // by a standard terminating newline — matching how git counts lines.
    let raw = repo
        .git(&["show", &format!("{}:{}", sha, file)])
        .unwrap_or_else(|e| panic!("{}: git show {}:{} failed: {}", ctx, sha, file, e));
    let mut lines: Vec<String> = raw.split('\n').map(|s| s.to_string()).collect();
    if lines.last().map(|s| s.is_empty()).unwrap_or(false) {
        lines.pop(); // strip artifact of trailing \n; double \n\n stays as one empty line
    }
    (line_authors, lines)
}

// ============================================================================
// END Category 3: Human Conflict Resolution
// ============================================================================

// ============================================================================
// Category 4: Conflict Resolved by AI
// Feature branch has AI-generated changes that conflict with main branch.
// AI resolves via set_contents with .ai() lines (writes + stages + checkpoints).
// The resolved lines gain AI attribution; surrounding human lines keep human
// attribution.  All other AI files in the chain retain their attribution.
// ============================================================================

#[derive(Clone, Copy)]
enum HumanContextAttribution {
    Known,
    Unattributed,
}

impl HumanContextAttribution {
    fn expected_line(self, contents: &str) -> ExpectedLine {
        match self {
            Self::Known => contents.human(),
            Self::Unattributed => contents.unattributed_human(),
        }
    }

    fn assert_metadata_humans(self, repo: &TestRepo, commit: &str, context: &str) {
        if matches!(self, Self::Unattributed) {
            return;
        }

        let conflict_note = parse_note(repo, commit);
        assert!(
            conflict_note
                .metadata
                .humans
                .contains_key("h_e858f2c2faea28"),
            "{context} should have h_e858f2c2faea28 in metadata.humans (human context lines in resolved file)"
        );
        assert_eq!(
            conflict_note.metadata.humans["h_e858f2c2faea28"].author,
            "Test User <test@example.com>"
        );
    }
}

mod ai_conflict_error_handling;
mod ai_conflict_human_context;
mod ai_conflict_multiple_files;
mod ai_conflict_on_first_commit;
mod ai_conflict_on_last_commit;
mod ai_conflict_rust_struct_fields;
mod ai_conflict_subsequent_edits;
mod ai_conflict_timeout_constant;
mod ai_conflict_with_added_extra_lines;
mod conflict_attribution_sources;
mod fast_path_go_handlers;
mod fast_path_growing_file;
mod fast_path_javascript_utilities;
mod fast_path_mixed_authors;
mod fast_path_multi_file_commits;
mod fast_path_nested_directories;
mod fast_path_python_service;
mod fast_path_recreated_file;
mod fast_path_rust_modules;
mod fast_path_typescript_components;
mod human_conflict_long_chain;
mod human_conflict_note_preservation;
mod human_conflict_python;
mod human_conflict_rust;
mod human_conflict_typescript;
mod slow_path_config_sections;
mod slow_path_function_offsets;
mod slow_path_growing_and_unique_files;
mod slow_path_growing_file;
mod slow_path_mixed_authors;
mod slow_path_mixed_files;
mod slow_path_python_utils;
mod slow_path_rust_impls;
mod slow_path_shared_files;
mod slow_path_typescript_routes;
