use std::collections::HashMap;

use serde::Serialize;

use crate::clients::auth::CredentialStore;
use crate::error::GitAiError;
use crate::model::authorship_log::PromptRecord;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::git::repository::Repository;

/// Metadata about user's auth state and git identity
#[derive(Debug, Serialize)]
struct BlameMetadata {
    is_logged_in: bool,
    current_user: Option<String>,
}

/// JSON output structure for blame
#[derive(Debug, Serialize)]
struct JsonBlameOutput {
    lines: std::collections::BTreeMap<String, String>,
    prompts: HashMap<String, PromptRecordWithOtherFiles>,
    metadata: BlameMetadata,
}

/// Read model that patches PromptRecord with other_files and commits fields
#[derive(Debug, Serialize)]
struct PromptRecordWithOtherFiles {
    #[serde(flatten)]
    prompt_record: PromptRecord,
    other_files: Vec<String>,
    commits: Vec<String>,
}

/// Helper function to get all files touched by a prompt hash across authorship logs
pub(super) fn get_files_for_prompt_hash(
    prompt_hash: &str,
    authorship_logs: &[AuthorshipLog],
    exclude_file: &str,
) -> Vec<String> {
    let mut files = std::collections::HashSet::new();

    for log in authorship_logs {
        for file_attestation in &log.attestations {
            // Skip the file we're currently blaming
            if file_attestation.file_path == exclude_file {
                continue;
            }

            // Check if any entry in this file has the prompt hash
            let has_hash = file_attestation
                .entries
                .iter()
                .any(|entry| entry.hash == prompt_hash);

            if has_hash {
                files.insert(file_attestation.file_path.clone());
            }
        }
    }

    let mut file_vec: Vec<String> = files.into_iter().collect();
    file_vec.sort();
    file_vec
}

pub(super) fn output_json_format(
    repo: &Repository,
    line_authors: &HashMap<u32, String>,
    prompt_records: &HashMap<String, PromptRecord>,
    authorship_logs: &[AuthorshipLog],
    prompt_commits: &HashMap<String, Vec<String>>,
    current_file: &str,
) -> Result<(), GitAiError> {
    // Filter to only AI lines (where author is a prompt_id in prompt_records)
    let mut ai_lines: Vec<(u32, String)> = line_authors
        .iter()
        .filter(|(_, author)| prompt_records.contains_key(*author))
        .map(|(line, author)| (*line, author.clone()))
        .collect();

    // Sort by line number
    ai_lines.sort_by_key(|(line, _)| *line);

    // Group consecutive lines with the same prompt_id into ranges
    let mut lines_map: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();

    if !ai_lines.is_empty() {
        let mut range_start = ai_lines[0].0;
        let mut range_end = ai_lines[0].0;
        let mut current_prompt_id = ai_lines[0].1.clone();

        for (line, prompt_id) in ai_lines.iter().skip(1) {
            if *prompt_id == current_prompt_id && *line == range_end + 1 {
                // Extend current range
                range_end = *line;
            } else {
                // Save current range and start new one
                let range_key = if range_start == range_end {
                    range_start.to_string()
                } else {
                    format!("{}-{}", range_start, range_end)
                };
                lines_map.insert(range_key, current_prompt_id.clone());

                range_start = *line;
                range_end = *line;
                current_prompt_id = prompt_id.clone();
            }
        }

        // Don't forget the last range
        let range_key = if range_start == range_end {
            range_start.to_string()
        } else {
            format!("{}-{}", range_start, range_end)
        };
        lines_map.insert(range_key, current_prompt_id);
    }

    // Only include prompts that are actually referenced in lines
    let referenced_prompt_ids: std::collections::HashSet<&String> = lines_map.values().collect();

    // Create read models with other_files and commits populated
    let filtered_prompts: HashMap<String, PromptRecordWithOtherFiles> = prompt_records
        .iter()
        .filter(|(k, _)| referenced_prompt_ids.contains(k))
        .map(|(k, v)| {
            let other_files = get_files_for_prompt_hash(k, authorship_logs, current_file);
            let commits = prompt_commits.get(k).cloned().unwrap_or_default();
            (
                k.clone(),
                PromptRecordWithOtherFiles {
                    prompt_record: v.clone(),
                    other_files,
                    commits,
                },
            )
        })
        .collect();

    // Compute metadata
    let is_logged_in = CredentialStore::new()
        .load()
        .ok()
        .flatten()
        .map(|creds| !creds.is_refresh_token_expired())
        .unwrap_or(false);

    let current_user = repo.effective_author_identity().formatted();

    let output = JsonBlameOutput {
        lines: lines_map,
        prompts: filtered_prompts,
        metadata: BlameMetadata {
            is_logged_in,
            current_user,
        },
    };

    let json_str = serde_json::to_string_pretty(&output)
        .map_err(|e| GitAiError::Generic(format!("Failed to serialize JSON output: {}", e)))?;

    println!("{}", json_str);
    Ok(())
}
