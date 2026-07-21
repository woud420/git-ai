//! Repository-aware line attribution lookup.
//!
//! Resolves the author and optional prompt for a given file/line of an
//! [`AuthorshipLog`]. The pure lookup over the log's own attestations lives in
//! `model`; this wrapper adds the repo-touching fallback (git-notes search for
//! prompt records not present locally), so the model stays free of `Repository`.

use crate::model::authorship_log::{Author, PromptRecord};
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::git::notes_api::{read_authorship, search_notes};
use crate::operations::git::repository::Repository;
use std::collections::HashMap;

/// Lookup the author and optional prompt for a given file and line.
///
/// Falls back to a git-notes search (cached in `foreign_prompts_cache`) when a
/// prompt hash is not present in the log's own metadata, e.g. after a
/// cherry-pick from another branch.
pub fn get_line_attribution(
    log: &AuthorshipLog,
    repo: &Repository,
    file: &str,
    line: u32,
    foreign_prompts_cache: &mut HashMap<String, Option<PromptRecord>>,
) -> Option<(Author, Option<String>, Option<PromptRecord>)> {
    // Find the file attestation
    let file_attestation = log.attestations.iter().find(|f| f.file_path == file)?;

    // Check entries in reverse order (latest wins)
    for entry in file_attestation.entries.iter().rev() {
        // Check if this line is covered by any of the line ranges
        let contains = entry.line_ranges.iter().any(|range| range.contains(line));
        if contains {
            // h_-prefixed hashes are known-human attestations — route to humans map
            if entry.hash.starts_with("h_") {
                if let Some(human_record) = log.metadata.humans.get(&entry.hash) {
                    return Some((
                        Author {
                            username: human_record.author.clone(),
                            email: String::new(),
                        },
                        Some(entry.hash.clone()),
                        None, // No PromptRecord for known-human lines
                    ));
                }
                // h_ hash not found locally (foreign cherry-pick) — skip this entry
                continue;
            }

            // s_-prefixed hashes are session attestations — route to sessions map
            if entry.hash.starts_with("s_") {
                // Extract session key from "s_<14hex>::t_<14hex>" format
                let session_key = entry.hash.split("::").next().unwrap_or(&entry.hash);
                if let Some(session_record) = log.metadata.sessions.get(session_key) {
                    // Create a PromptRecord-like structure from SessionRecord for compatibility
                    // Note: sessions don't have message transcripts or detailed stats
                    let prompt_record = PromptRecord {
                        agent_id: session_record.agent_id.clone(),
                        human_author: session_record.human_author.clone(),
                        total_additions: 0, // Sessions don't track detailed stats
                        total_deletions: 0,
                        accepted_lines: 0,
                        overriden_lines: 0,
                        custom_attributes: session_record.custom_attributes.clone(),
                        messages_url: None,
                    };
                    return Some((
                        Author {
                            username: session_record.agent_id.tool.clone(),
                            email: String::new(),
                        },
                        Some(entry.hash.clone()), // Return full s_::t_ hash
                        Some(prompt_record),
                    ));
                }
                // Session hash not found locally — skip this entry
                continue;
            }

            // The hash corresponds to a prompt session short hash
            if let Some(prompt_record) = log.metadata.prompts.get(&entry.hash) {
                // Create author info from the prompt record
                let author = Author {
                    username: prompt_record.agent_id.tool.clone(),
                    email: String::new(), // AI agents don't have email
                };

                // Return author and prompt info
                return Some((
                    author,
                    Some(entry.hash.clone()),
                    Some(prompt_record.clone()),
                ));
            } else {
                // Check cache first before grepping
                let prompt_record =
                    if let Some(cached_result) = foreign_prompts_cache.get(&entry.hash) {
                        cached_result.clone()
                    } else {
                        // Try to find prompt record using git grep
                        let shas =
                            search_notes(repo, &format!("\"{}\"", &entry.hash)).unwrap_or_default();
                        let result = if let Some(latest_sha) = shas.first() {
                            if let Some(authorship_log) = read_authorship(repo, latest_sha) {
                                authorship_log.metadata.prompts.get(&entry.hash).cloned()
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        // Cache the result (even if None) to avoid repeated grepping
                        foreign_prompts_cache.insert(entry.hash.clone(), result.clone());
                        result
                    };

                if let Some(prompt_record) = prompt_record {
                    let author = Author {
                        username: prompt_record.agent_id.tool.clone(),
                        email: String::new(), // AI agents don't have email
                    };
                    return Some((author, Some(entry.hash.clone()), Some(prompt_record)));
                }
            }
        }
    }
    None
}
