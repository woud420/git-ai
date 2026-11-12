//! `git-ai diff` command: argument parsing, orchestration, and public types.
//!
//! The heavy lifting is split across sibling modules (declared in the parent
//! `commands` module):
//! - `diff_parsing`      — raw `git diff` invocation and unified-diff parsing
//! - `diff_attribution`  — authorship overlay (blame / authorship-log paths)
//! - `diff_json_builder` — building the `--json` output structures
//! - `diff_render`       — ANSI terminal rendering

use crate::cli::fail::print_stdout;
use crate::error::GitAiError;
use crate::operations::git::repository::Repository;

// Re-export public items used by callers outside this module so the public API
// path (`operations::commands::diff::*`) stays unchanged.
pub use crate::model::diff_json::{
    Attribution, DiffBuildArtifacts, DiffCommitMetadata, DiffCommitStats, DiffJson, DiffJsonHunk,
    DiffLineKey, DiffToolModelStats, LineSide,
};
pub use crate::operations::commands::diff_args::parse_diff_args;
pub use crate::operations::commands::diff_attribution::{
    build_diff_artifacts, build_diff_artifacts_from_hunks, build_diff_artifacts_with_note,
};
pub use crate::operations::commands::diff_parsing::get_diff_with_line_numbers;
pub use crate::operations::commands::diff_render::format_annotated_diff;

// Internal imports (not re-exported; these were private fns in the old diff.rs).
use crate::operations::commands::diff_json_builder::{
    build_diff_json, calculate_diff_commit_stats,
    merge_missing_prompts_and_sessions_from_authorship_note,
};

// ============================================================================
// Data Structures (public contract — zero changes allowed here)
// ============================================================================

#[derive(Debug, Clone)]
pub enum DiffSpec {
    SingleCommit(String),      // SHA
    TwoCommit(String, String), // start..end
}

#[derive(Debug, Clone)]
pub enum DiffFormat {
    Json,
    GitCompatibleTerminal,
}

#[derive(Debug)]
pub struct DiffHunk {
    pub file_path: String,
    pub old_file_path: Option<String>,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub deleted_lines: Vec<u32>, // Absolute line numbers in OLD file
    pub added_lines: Vec<u32>,   // Absolute line numbers in NEW file
    pub deleted_contents: Vec<String>,
    pub added_contents: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DiffCommandOptions {
    pub format: DiffFormat,
    pub blame_deletions: bool,
    pub blame_deletions_since: Option<String>,
    pub include_stats: bool,
    pub all_prompts: bool,
}

impl Default for DiffCommandOptions {
    fn default() -> Self {
        Self {
            format: DiffFormat::GitCompatibleTerminal,
            blame_deletions: false,
            blame_deletions_since: None,
            include_stats: false,
            all_prompts: false,
        }
    }
}

#[derive(Debug)]
pub struct ParsedDiffArgs {
    pub spec: DiffSpec,
    pub options: DiffCommandOptions,
}

// ============================================================================
// Main Entry Point
// ============================================================================

pub fn handle_diff(repo: &Repository, args: &[String]) -> Result<(), GitAiError> {
    if args.is_empty() {
        eprintln!("Error: diff requires a commit or commit range argument");
        eprintln!("Usage: git-ai diff <commit>");
        eprintln!("       git-ai diff <commit1>..<commit2>");
        std::process::exit(1);
    }

    let parsed = parse_diff_args(args)?;
    let output = execute_diff(repo, parsed)?;
    print_stdout(&output);

    Ok(())
}

// ============================================================================
// Core Execution Logic
// ============================================================================

pub fn execute_diff(repo: &Repository, parsed: ParsedDiffArgs) -> Result<String, GitAiError> {
    let is_single_commit = matches!(&parsed.spec, DiffSpec::SingleCommit(_));

    // Resolve commits to get from/to SHAs.
    let (from_commit, to_commit) = match parsed.spec {
        DiffSpec::TwoCommit(start, end) => {
            let from = resolve_commit(repo, &start)?;
            let to = resolve_commit(repo, &end)?;
            (from, to)
        }
        DiffSpec::SingleCommit(commit) => {
            let to = resolve_commit(repo, &commit)?;
            let from = resolve_parent(repo, &to)?;
            (from, to)
        }
    };

    // Build a single set of artifacts used by both terminal and JSON outputs.
    let artifacts = build_diff_artifacts(repo, &from_commit, &to_commit, &parsed.options)?;

    // Format and output annotated diff.
    let output = match parsed.options.format {
        DiffFormat::Json => {
            let mut output_prompts = artifacts.prompts.clone();
            let mut output_sessions = artifacts.sessions.clone();
            if is_single_commit && parsed.options.all_prompts {
                merge_missing_prompts_and_sessions_from_authorship_note(
                    repo,
                    &to_commit,
                    &mut output_prompts,
                    &mut output_sessions,
                );
            }

            let commit_stats = if parsed.options.include_stats {
                let mut stats_prompts = output_prompts.clone();
                let mut stats_sessions = output_sessions.clone();
                if is_single_commit && !parsed.options.all_prompts {
                    merge_missing_prompts_and_sessions_from_authorship_note(
                        repo,
                        &to_commit,
                        &mut stats_prompts,
                        &mut stats_sessions,
                    );
                }
                Some(calculate_diff_commit_stats(
                    &artifacts,
                    &stats_prompts,
                    &stats_sessions,
                ))
            } else {
                None
            };

            let diff_json = build_diff_json(
                repo,
                &from_commit,
                &to_commit,
                &artifacts,
                &output_prompts,
                &output_sessions,
                commit_stats,
            )?;
            serde_json::to_string(&diff_json)
                .map_err(|e| GitAiError::Generic(format!("Failed to serialize JSON: {}", e)))?
        }
        DiffFormat::GitCompatibleTerminal => format_annotated_diff(
            repo,
            &from_commit,
            &to_commit,
            &artifacts.attributions,
            &artifacts.humans,
            &artifacts.included_files,
        )?,
    };

    Ok(output)
}

// ============================================================================
// Commit Resolution
// ============================================================================

fn resolve_commit(repo: &Repository, rev: &str) -> Result<String, GitAiError> {
    use crate::clients::git_cli::{InternalGitProfile, exec_git_with_profile};

    let mut args = repo.global_args_for_exec();
    args.push("rev-parse".to_string());
    args.push(rev.to_string());

    let output = exec_git_with_profile(&args, InternalGitProfile::General)?;
    let sha = String::from_utf8(output.stdout)
        .map_err(|e| GitAiError::Generic(format!("Failed to parse rev-parse output: {}", e)))?
        .trim()
        .to_string();

    if sha.is_empty() {
        return Err(GitAiError::Generic(format!(
            "Could not resolve commit: {}",
            rev
        )));
    }

    Ok(sha)
}

fn resolve_parent(repo: &Repository, commit: &str) -> Result<String, GitAiError> {
    use crate::clients::git_cli::{InternalGitProfile, exec_git_with_profile};

    let parent_rev = format!("{}^", commit);

    let mut args = repo.global_args_for_exec();
    args.push("rev-parse".to_string());
    args.push(parent_rev);

    let output = exec_git_with_profile(&args, InternalGitProfile::General);

    match output {
        Ok(out) => {
            let sha = String::from_utf8(out.stdout)
                .map_err(|e| GitAiError::Generic(format!("Failed to parse parent SHA: {}", e)))?
                .trim()
                .to_string();

            if sha.is_empty() {
                // No parent — initial commit; use empty tree.
                Ok("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string())
            } else {
                Ok(sha)
            }
        }
        Err(_) => {
            // No parent — initial commit; use empty tree hash.
            Ok("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string())
        }
    }
}

// ============================================================================
// Filtered Diff for Bundle Sharing
// ============================================================================

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_line_key_equality() {
        let key1 = DiffLineKey {
            file: "test.rs".to_string(),
            line: 10,
            side: LineSide::Old,
        };

        let key2 = DiffLineKey {
            file: "test.rs".to_string(),
            line: 10,
            side: LineSide::Old,
        };

        let key3 = DiffLineKey {
            file: "test.rs".to_string(),
            line: 10,
            side: LineSide::New,
        };

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }
}
