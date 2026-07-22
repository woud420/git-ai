//! `git-ai diff` command: argument parsing, orchestration, and public types.
//!
//! The heavy lifting is split across sibling modules (declared in the parent
//! `commands` module):
//! - `diff_parsing`      — raw `git diff` invocation and unified-diff parsing
//! - `diff_attribution`  — authorship overlay (blame / authorship-log paths)
//! - `diff_json_builder` — building the `--json` output structures
//! - `diff_render`       — ANSI terminal rendering

use crate::error::GitAiError;
use crate::model::authorship_log::{HumanRecord, PromptRecord, SessionRecord};
use crate::model::diff_json::FileDiffJson;
use crate::operations::git::repository::Repository;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

// Re-export public items used by callers outside this module so the public API
// path (`operations::commands::diff::*`) stays unchanged.
pub use crate::operations::commands::diff_args::parse_diff_args;
pub use crate::operations::commands::diff_attribution::{
    build_diff_artifacts, build_diff_artifacts_from_hunks, build_diff_artifacts_with_note,
    overlay_diff_attributions,
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

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub struct DiffLineKey {
    pub file: String,
    pub line: u32,
    pub side: LineSide,
}

/// JSON output format for `git-ai diff --json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffJson {
    /// Per-file diff information with annotations
    pub files: BTreeMap<String, FileDiffJson>,
    /// Prompt records keyed by prompt hash (old-format, bare 16-char hex)
    pub prompts: BTreeMap<String, PromptRecord>,
    /// Session records keyed by full attestation hash (s_xxx::t_yyy)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sessions: BTreeMap<String, SessionRecord>,
    /// Human records keyed by human hash (h_-prefixed)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub humans: BTreeMap<String, HumanRecord>,
    /// Per-hunk records for machine consumption
    #[serde(default)]
    pub hunks: Vec<DiffJsonHunk>,
    /// Commit metadata for all commits referenced by hunks
    #[serde(default)]
    pub commits: BTreeMap<String, DiffCommitMetadata>,
    /// Optional commit stats for single-commit diffs (`--json --include-stats`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_stats: Option<DiffCommitStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DiffToolModelStats {
    #[serde(default)]
    pub ai_lines_added: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DiffCommitStats {
    #[serde(default)]
    pub ai_lines_added: u32,
    #[serde(default)]
    pub human_lines_added: u32,
    #[serde(default)]
    pub unknown_lines_added: u32,
    #[serde(default)]
    pub git_lines_added: u32,
    #[serde(default)]
    pub git_lines_deleted: u32,
    #[serde(default)]
    pub tool_model_breakdown: BTreeMap<String, DiffToolModelStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffJsonHunk {
    pub commit_sha: String,
    pub content_hash: String,
    pub hunk_kind: String, // "addition" | "deletion"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_commit_sha: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffCommitMetadata {
    pub authored_time: String,
    pub msg: String,
    pub full_msg: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorship_note: Option<String>,
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum LineSide {
    Old, // For deleted lines
    New, // For added lines
}

#[derive(Debug, Clone)]
pub enum Attribution {
    Ai(String),    // Tool name: "cursor", "claude", etc.
    Human(String), // Username
    NoData,        // No authorship data available
}

#[derive(Debug)]
pub struct DiffBuildArtifacts {
    pub attributions: HashMap<DiffLineKey, Attribution>,
    pub annotations_by_file:
        BTreeMap<String, BTreeMap<String, Vec<crate::model::authorship_log::LineRange>>>,
    pub prompts: BTreeMap<String, PromptRecord>,
    pub sessions: BTreeMap<String, SessionRecord>,
    pub humans: BTreeMap<String, HumanRecord>,
    pub json_hunks: Vec<DiffJsonHunk>,
    pub commits: BTreeMap<String, DiffCommitMetadata>,
    pub included_files: HashSet<String>,
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
    print!("{}", output);

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

/// Options for getting a diff with optional filtering.
#[derive(Default)]
pub struct DiffOptions {
    /// If provided, only include files with attributions from these prompts.
    pub prompt_ids: Option<Vec<String>>,
    /// Whether to filter files to only those with attributions from `prompt_ids`.
    pub filter_to_attributed_files: bool,
}

/// Get diff JSON for a single commit with optional filtering by prompt attributions.
///
/// This function is designed for bundle sharing:
/// - If `options.filter_to_attributed_files` is true, only includes files that have
///   attributions from the specified `prompt_ids`.
/// - If `options.prompt_ids` is `Some`, filters the returned prompts to only those IDs.
pub fn get_diff_json_filtered(
    repo: &Repository,
    commit_sha: &str,
    options: DiffOptions,
) -> Result<DiffJson, GitAiError> {
    use crate::operations::commands::diff_json_builder::extract_session_id;

    // Resolve the commit to get from/to SHAs (parent -> commit)
    let to_commit = resolve_commit(repo, commit_sha)?;
    let from_commit = resolve_parent(repo, &to_commit)?;

    let artifacts = build_diff_artifacts(
        repo,
        &from_commit,
        &to_commit,
        &DiffCommandOptions {
            format: DiffFormat::Json,
            ..DiffCommandOptions::default()
        },
    )?;

    let mut diff_json = build_diff_json(
        repo,
        &from_commit,
        &to_commit,
        &artifacts,
        &artifacts.prompts,
        &artifacts.sessions,
        None,
    )?;

    // Apply filtering if requested
    if options.filter_to_attributed_files
        && let Some(ref prompt_ids) = options.prompt_ids
    {
        let prompt_id_set: HashSet<&String> = prompt_ids.iter().collect();

        // Filter files to only those with attributions from the specified prompts
        diff_json.files.retain(|_file_path, file_diff| {
            // Check if any annotation key matches a prompt_id
            file_diff
                .annotations
                .keys()
                .any(|key| prompt_id_set.contains(key))
        });

        let kept_files: HashSet<String> = diff_json.files.keys().cloned().collect();
        diff_json
            .hunks
            .retain(|hunk| kept_files.contains(&hunk.file_path));
    }

    // Filter prompts/sessions to only those specified (if any)
    if let Some(ref prompt_ids) = options.prompt_ids {
        let prompt_id_set: HashSet<&String> = prompt_ids.iter().collect();
        diff_json
            .prompts
            .retain(|key, _| prompt_id_set.contains(key));
        // Session keys are session IDs only, but prompt_ids may contain combined IDs
        // Extract session IDs from prompt_ids for session filtering
        let session_id_set: HashSet<&str> =
            prompt_ids.iter().map(|id| extract_session_id(id)).collect();
        diff_json
            .sessions
            .retain(|key, _| session_id_set.contains(key.as_str()));
    }

    let mut referenced_commit_shas: HashSet<String> = HashSet::new();
    for hunk in &diff_json.hunks {
        referenced_commit_shas.insert(hunk.commit_sha.clone());
        if let Some(original) = &hunk.original_commit_sha {
            referenced_commit_shas.insert(original.clone());
        }
    }
    diff_json
        .commits
        .retain(|sha, _| referenced_commit_shas.contains(sha));

    Ok(diff_json)
}

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
