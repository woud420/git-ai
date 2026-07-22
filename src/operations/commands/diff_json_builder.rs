//! Building the JSON output structures for `git-ai diff --json`.
//!
//! This module takes [`DiffBuildArtifacts`] (from `diff_attribution`) and
//! produces the serialisable [`DiffJson`] and related types.

use crate::clients::git_cli::{InternalGitProfile, exec_git_with_profile};
use crate::error::GitAiError;
use crate::model::authorship_log::{LineRange, PromptRecord, SessionRecord};
use crate::model::diff_json::FileDiffJson;
use crate::operations::git::notes_api::{read_authorship, read_note};
use crate::operations::git::repository::Repository;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

use crate::operations::commands::diff::{
    Attribution, DiffBuildArtifacts, DiffCommitMetadata, DiffCommitStats, DiffHunk, DiffJson,
    DiffJsonHunk, DiffLineKey, LineSide,
};
use crate::operations::commands::diff_attribution::LineAttributionDetail;
use crate::operations::commands::diff_parsing::get_diff_sections_by_file;

// ============================================================================
// JSON top-level builder
// ============================================================================

/// Build the [`DiffJson`] structure for `--json` output.
pub(crate) fn build_diff_json(
    repo: &Repository,
    from_commit: &str,
    to_commit: &str,
    artifacts: &DiffBuildArtifacts,
    prompts: &BTreeMap<String, PromptRecord>,
    sessions: &BTreeMap<String, SessionRecord>,
    commit_stats: Option<DiffCommitStats>,
) -> Result<DiffJson, GitAiError> {
    let mut files: BTreeMap<String, FileDiffJson> = BTreeMap::new();
    let file_diffs = get_diff_split_by_file(repo, from_commit, to_commit)?;
    let mut files_sorted: Vec<&String> = artifacts.included_files.iter().collect();
    files_sorted.sort();

    for file_path in files_sorted {
        let diff = file_diffs.get(file_path).cloned().unwrap_or_default();

        let base_content = match repo.get_file_content(file_path, from_commit) {
            Ok(bytes) => String::from_utf8(bytes).unwrap_or_default(),
            Err(_) => String::new(),
        };
        let annotations = artifacts
            .annotations_by_file
            .get(file_path)
            .cloned()
            .unwrap_or_default();

        files.insert(
            file_path.clone(),
            FileDiffJson {
                annotations,
                diff,
                base_content,
            },
        );
    }

    Ok(DiffJson {
        files,
        prompts: prompts.clone(),
        sessions: sessions.clone(),
        humans: artifacts.humans.clone(),
        hunks: artifacts.json_hunks.clone(),
        commits: artifacts.commits.clone(),
        commit_stats,
    })
}

/// Calculate aggregated statistics for a single-commit diff.
pub(crate) fn calculate_diff_commit_stats(
    artifacts: &DiffBuildArtifacts,
    prompts: &BTreeMap<String, PromptRecord>,
    sessions: &BTreeMap<String, SessionRecord>,
) -> DiffCommitStats {
    let mut stats = DiffCommitStats::default();

    for annotations in artifacts.annotations_by_file.values() {
        for (prompt_id, ranges) in annotations {
            let landed_lines = ranges.iter().map(line_range_len).sum::<u32>();
            stats.ai_lines_added += landed_lines;
            let session_key = extract_session_id(prompt_id);
            let key = prompts
                .get(prompt_id)
                .map(|r| &r.agent_id)
                .or_else(|| sessions.get(session_key).map(|r| &r.agent_id))
                .map(|agent_id| format!("{}::{}", agent_id.tool, agent_id.model));
            if let Some(key) = key {
                let tool_stats = stats.tool_model_breakdown.entry(key).or_default();
                tool_stats.ai_lines_added += landed_lines;
            }
        }
    }

    for (line_key, attribution) in &artifacts.attributions {
        if !matches!(line_key.side, LineSide::New) {
            continue;
        }
        match attribution {
            Attribution::Human(_) => stats.human_lines_added += 1,
            Attribution::NoData => stats.unknown_lines_added += 1,
            Attribution::Ai(_) => {}
        }
    }
    stats.git_lines_added =
        stats.ai_lines_added + stats.human_lines_added + stats.unknown_lines_added;

    for hunk in &artifacts.json_hunks {
        if hunk.hunk_kind == "deletion" {
            stats.git_lines_deleted += hunk.end_line.saturating_sub(hunk.start_line) + 1;
        }
    }

    stats
}

pub(crate) fn merge_missing_prompts_and_sessions_from_authorship_note(
    repo: &Repository,
    commit_sha: &str,
    prompts: &mut BTreeMap<String, PromptRecord>,
    sessions: &mut BTreeMap<String, SessionRecord>,
) {
    if let Some(authorship_log) = read_authorship(repo, commit_sha) {
        for (prompt_id, prompt_record) in &authorship_log.metadata.prompts {
            prompts
                .entry(prompt_id.clone())
                .or_insert_with(|| prompt_record.clone());
        }
        // Insert session records keyed by session ID only (s_xxx).
        for file_attestation in &authorship_log.attestations {
            for entry in &file_attestation.entries {
                if entry.hash.starts_with("s_") {
                    let session_key = extract_session_id(&entry.hash);
                    if let Some(session_record) = authorship_log.metadata.sessions.get(session_key)
                    {
                        sessions
                            .entry(session_key.to_string())
                            .or_insert_with(|| session_record.clone());
                    }
                }
            }
        }
    }
}

// ============================================================================
// JSON hunk building
// ============================================================================

pub fn build_json_hunks(
    repo: &Repository,
    diff_hunks: &[DiffHunk],
    line_details: &HashMap<DiffLineKey, LineAttributionDetail>,
    line_contents: &HashMap<DiffLineKey, String>,
    diff_to_commit: &str,
    commits: &mut BTreeMap<String, DiffCommitMetadata>,
) -> Result<Vec<DiffJsonHunk>, GitAiError> {
    let mut hunks: Vec<DiffJsonHunk> = Vec::new();

    for diff_hunk in diff_hunks {
        hunks.extend(build_json_hunk_segments(
            repo,
            diff_hunk,
            LineSide::New,
            "addition",
            diff_to_commit,
            line_details,
            line_contents,
            commits,
        )?);
        hunks.extend(build_json_hunk_segments(
            repo,
            diff_hunk,
            LineSide::Old,
            "deletion",
            diff_to_commit,
            line_details,
            line_contents,
            commits,
        )?);
    }

    Ok(hunks)
}

#[allow(clippy::too_many_arguments)]
fn build_json_hunk_segments(
    repo: &Repository,
    diff_hunk: &DiffHunk,
    side: LineSide,
    kind: &str,
    diff_to_commit: &str,
    line_details: &HashMap<DiffLineKey, LineAttributionDetail>,
    line_contents: &HashMap<DiffLineKey, String>,
    commits: &mut BTreeMap<String, DiffCommitMetadata>,
) -> Result<Vec<DiffJsonHunk>, GitAiError> {
    let lines = match side {
        LineSide::Old => &diff_hunk.deleted_lines,
        LineSide::New => &diff_hunk.added_lines,
    };
    if lines.is_empty() {
        return Ok(Vec::new());
    }

    let mut segments: Vec<DiffJsonHunk> = Vec::new();
    let mut current_start = 0u32;
    let mut current_end = 0u32;
    let mut current_prompt_id: Option<String> = None;
    let mut current_human_id: Option<String> = None;
    let mut current_original_commit_sha: Option<String> = None;
    let mut current_commit_sha = String::new();
    let mut current_contents: Vec<String> = Vec::new();

    let flush = |segments: &mut Vec<DiffJsonHunk>,
                 current_start: &mut u32,
                 current_end: &mut u32,
                 current_prompt_id: &mut Option<String>,
                 current_human_id: &mut Option<String>,
                 current_original_commit_sha: &mut Option<String>,
                 current_commit_sha: &mut String,
                 current_contents: &mut Vec<String>| {
        if *current_start == 0 {
            return;
        }
        let content_hash = hash_hunk_content(current_contents);
        let session_id = current_prompt_id.as_ref().and_then(|id| {
            if id.starts_with("s_") {
                Some(extract_session_id(id).to_string())
            } else {
                None
            }
        });
        segments.push(DiffJsonHunk {
            commit_sha: current_commit_sha.clone(),
            content_hash,
            hunk_kind: kind.to_string(),
            original_commit_sha: current_original_commit_sha.clone(),
            start_line: *current_start,
            end_line: *current_end,
            file_path: diff_hunk.file_path.clone(),
            prompt_id: current_prompt_id.clone(),
            session_id,
            human_id: current_human_id.clone(),
        });
        *current_start = 0;
        *current_end = 0;
        *current_prompt_id = None;
        *current_human_id = None;
        *current_original_commit_sha = None;
        current_commit_sha.clear();
        current_contents.clear();
    };

    for line in lines {
        let key = DiffLineKey {
            file: diff_hunk.file_path.clone(),
            line: *line,
            side: side.clone(),
        };
        let detail = line_details.get(&key);
        let prompt_id = detail.and_then(|d| d.prompt_id.clone());
        let human_id = detail.and_then(|d| d.human_id.clone());
        let original_commit_sha = if matches!(side, LineSide::Old) {
            detail.and_then(|d| d.commit_sha.clone())
        } else {
            None
        };
        let commit_sha = if matches!(side, LineSide::Old) {
            diff_to_commit.to_string()
        } else {
            detail
                .and_then(|d| d.commit_sha.clone())
                .unwrap_or_else(|| diff_to_commit.to_string())
        };

        if let Some(ref original_sha) = original_commit_sha {
            ensure_commit_metadata(repo, original_sha, commits);
        }
        ensure_commit_metadata(repo, &commit_sha, commits);

        let can_extend = current_start != 0
            && *line == current_end + 1
            && prompt_id == current_prompt_id
            && human_id == current_human_id
            && original_commit_sha == current_original_commit_sha
            && commit_sha == current_commit_sha;

        if !can_extend {
            flush(
                &mut segments,
                &mut current_start,
                &mut current_end,
                &mut current_prompt_id,
                &mut current_human_id,
                &mut current_original_commit_sha,
                &mut current_commit_sha,
                &mut current_contents,
            );
            current_start = *line;
            current_end = *line;
            current_prompt_id = prompt_id.clone();
            current_human_id = human_id.clone();
            current_original_commit_sha = original_commit_sha.clone();
            current_commit_sha = commit_sha;
        } else {
            current_end = *line;
        }

        current_contents.push(line_contents.get(&key).cloned().unwrap_or_default());
    }

    flush(
        &mut segments,
        &mut current_start,
        &mut current_end,
        &mut current_prompt_id,
        &mut current_human_id,
        &mut current_original_commit_sha,
        &mut current_commit_sha,
        &mut current_contents,
    );

    Ok(segments)
}

fn hash_hunk_content(lines: &[String]) -> String {
    let joined = lines.join("\n");
    let mut hasher = Sha256::new();
    hasher.update(joined.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(crate) fn ensure_commit_metadata(
    repo: &Repository,
    commit_sha: &str,
    commits: &mut BTreeMap<String, DiffCommitMetadata>,
) {
    if commits.contains_key(commit_sha) {
        return;
    }
    if let Ok(metadata) = load_commit_metadata(repo, commit_sha) {
        commits.insert(commit_sha.to_string(), metadata);
    }
}

fn load_commit_metadata(
    repo: &Repository,
    commit_sha: &str,
) -> Result<DiffCommitMetadata, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("show".to_string());
    args.push("-s".to_string());
    args.push("--no-notes".to_string());
    args.push("--encoding=UTF-8".to_string());
    args.push("--format=%an%x00%ae%x00%aI%x00%s%x00%B".to_string());
    args.push(commit_sha.to_string());

    let output = exec_git_with_profile(&args, InternalGitProfile::General)?;
    let stdout = String::from_utf8(output.stdout)
        .map_err(|e| GitAiError::Generic(format!("Failed to parse commit metadata: {}", e)))?;
    let mut parts = stdout.splitn(5, '\0');
    let author_name = parts.next().unwrap_or("").trim();
    let author_email = parts.next().unwrap_or("").trim();
    let authored_time = parts.next().unwrap_or("").trim().to_string();
    let msg = parts.next().unwrap_or("").trim().to_string();
    let full_msg = parts.next().unwrap_or("").trim_end().to_string();
    let author = format_git_ident(author_name, author_email);
    let authorship_note = read_note(repo, commit_sha);

    Ok(DiffCommitMetadata {
        authored_time,
        msg,
        full_msg,
        author,
        authorship_note,
    })
}

fn format_git_ident(name: &str, email: &str) -> String {
    if !name.is_empty() && !email.is_empty() {
        format!("{} <{}>", name, email)
    } else if !name.is_empty() {
        name.to_string()
    } else if !email.is_empty() {
        format!("<{}>", email)
    } else {
        String::new()
    }
}

// ============================================================================
// Helpers shared with diff_attribution
// ============================================================================

/// Extract session ID from a combined session::trace ID.
/// For `"s_xxx::t_yyy"` returns `"s_xxx"`.  For other IDs returns `id` unchanged.
pub fn extract_session_id(id: &str) -> &str {
    if id.starts_with("s_") {
        id.split("::").next().unwrap_or(id)
    } else {
        id
    }
}

fn line_range_len(range: &LineRange) -> u32 {
    match range {
        LineRange::Single(_) => 1,
        LineRange::Range(start, end) => end.saturating_sub(*start) + 1,
    }
}

// ============================================================================
// Diff-split-by-file helper (needed by build_diff_json)
// ============================================================================

fn get_diff_split_by_file(
    repo: &Repository,
    from_commit: &str,
    to_commit: &str,
) -> Result<HashMap<String, String>, GitAiError> {
    let sections = get_diff_sections_by_file(repo, from_commit, to_commit)?;
    let mut file_diffs: HashMap<String, String> = HashMap::new();
    for (file_path, diff_text) in sections {
        file_diffs.insert(file_path, diff_text);
    }
    Ok(file_diffs)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::authorship_log::LineRange;
    use crate::model::working_log::AgentId;
    use crate::operations::commands::diff::{
        Attribution, DiffBuildArtifacts, DiffJsonHunk, DiffLineKey, LineSide,
    };
    use std::collections::{BTreeMap, HashMap, HashSet};

    #[test]
    fn test_calculate_diff_commit_stats_tracks_unknown_added_lines() {
        fn prompt_record(tool: &str, model: &str, additions: u32, deletions: u32) -> PromptRecord {
            PromptRecord {
                agent_id: AgentId {
                    tool: tool.to_string(),
                    id: format!("{}-id", tool),
                    model: model.to_string(),
                },
                human_author: None,
                total_additions: additions,
                total_deletions: deletions,
                accepted_lines: 0,
                overriden_lines: 0,
                custom_attributes: None,
                messages_url: None,
            }
        }

        let mut attributions = HashMap::new();
        attributions.insert(
            DiffLineKey {
                file: "f.rs".to_string(),
                line: 1,
                side: LineSide::New,
            },
            Attribution::Ai("cursor".to_string()),
        );
        attributions.insert(
            DiffLineKey {
                file: "f.rs".to_string(),
                line: 2,
                side: LineSide::New,
            },
            Attribution::Human("alice".to_string()),
        );
        attributions.insert(
            DiffLineKey {
                file: "f.rs".to_string(),
                line: 3,
                side: LineSide::New,
            },
            Attribution::NoData,
        );
        // Old-side no-data should not affect unknown_lines_added.
        attributions.insert(
            DiffLineKey {
                file: "f.rs".to_string(),
                line: 10,
                side: LineSide::Old,
            },
            Attribution::NoData,
        );

        let mut annotations = BTreeMap::new();
        annotations.insert("p1".to_string(), vec![LineRange::Single(1)]);
        let mut annotations_by_file = BTreeMap::new();
        annotations_by_file.insert("f.rs".to_string(), annotations);

        let mut prompts = BTreeMap::new();
        prompts.insert("p1".to_string(), prompt_record("cursor", "gpt-4o", 5, 2));

        let artifacts = DiffBuildArtifacts {
            attributions,
            annotations_by_file,
            prompts: prompts.clone(),
            humans: BTreeMap::new(),
            sessions: BTreeMap::new(),
            json_hunks: vec![DiffJsonHunk {
                commit_sha: "abc".to_string(),
                content_hash: "hash".to_string(),
                hunk_kind: "deletion".to_string(),
                original_commit_sha: None,
                start_line: 5,
                end_line: 6,
                file_path: "f.rs".to_string(),
                prompt_id: None,
                session_id: None,
                human_id: None,
            }],
            commits: BTreeMap::new(),
            included_files: HashSet::new(),
        };

        let sessions: BTreeMap<String, SessionRecord> = BTreeMap::new();
        let stats = calculate_diff_commit_stats(&artifacts, &prompts, &sessions);
        assert_eq!(stats.ai_lines_added, 1);
        assert_eq!(stats.human_lines_added, 1);
        assert_eq!(stats.unknown_lines_added, 1);
        assert_eq!(stats.git_lines_added, 3);
        assert_eq!(stats.git_lines_deleted, 2);

        let breakdown = stats
            .tool_model_breakdown
            .get("cursor::gpt-4o")
            .expect("expected cursor::gpt-4o breakdown entry");
        assert_eq!(breakdown.ai_lines_added, 1);
    }

    #[test]
    fn test_format_git_ident_prefers_full_ident() {
        assert_eq!(
            format_git_ident("Test User", "test@example.com"),
            "Test User <test@example.com>"
        );
    }

    #[test]
    fn test_format_git_ident_handles_missing_parts() {
        assert_eq!(format_git_ident("Test User", ""), "Test User");
        assert_eq!(
            format_git_ident("", "test@example.com"),
            "<test@example.com>"
        );
        assert_eq!(format_git_ident("", ""), "");
    }
}
