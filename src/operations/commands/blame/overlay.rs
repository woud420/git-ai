use std::collections::{BTreeMap, HashMap, HashSet};

use crate::error::GitAiError;
use crate::model::authorship_log::{HumanRecord, PromptRecord, SessionRecord};
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::model::working_log::CheckpointKind;
use crate::operations::authorship::line_lookup::get_line_attribution;
use crate::operations::git::notes_api::read_authorship_v3;
use crate::operations::git::repository::Repository;

use super::{BlameHunk, GitAiBlameOptions};

#[allow(clippy::type_complexity)]
pub(super) fn overlay_ai_authorship(
    repo: &Repository,
    blame_hunks: &[BlameHunk],
    file_path: &str,
    options: &GitAiBlameOptions,
) -> Result<
    (
        HashMap<u32, String>,
        HashMap<String, PromptRecord>,
        HashMap<String, SessionRecord>,
        BTreeMap<String, HumanRecord>, // humans map
        Vec<AuthorshipLog>,
        HashMap<String, Vec<String>>, // prompt_hash -> commit_shas
        HashSet<String>,              // commit SHAs with real authorship notes
    ),
    GitAiError,
> {
    let mut line_authors: HashMap<u32, String> = HashMap::new();
    let mut prompt_records: HashMap<String, PromptRecord> = HashMap::new();
    let mut session_records: HashMap<String, SessionRecord> = HashMap::new();
    let mut humans: BTreeMap<String, HumanRecord> = BTreeMap::new();
    // Track which commits contain each prompt hash
    let mut prompt_commits: HashMap<String, HashSet<String>> = HashMap::new();
    // Track commit SHAs that have real (non-simulated) authorship notes
    let mut commits_with_notes: HashSet<String> = HashSet::new();

    // Group hunks by commit SHA to avoid repeated lookups
    let mut commit_authorship_cache: HashMap<String, Option<AuthorshipLog>> = HashMap::new();
    // Simulated authorship logs for agent commits without notes. We keep these separate
    // from commit_authorship_cache so a single agent commit can be handled across multiple
    // blame hunks without being limited to the first hunk's line range.
    let mut simulated_authorship_logs: HashMap<String, AuthorshipLog> = HashMap::new();
    // Cache for foreign prompts to avoid repeated grepping
    let mut foreign_prompts_cache: HashMap<String, Option<PromptRecord>> = HashMap::new();
    for hunk in blame_hunks {
        // Check if we've already looked up this commit's authorship
        let authorship_log = if let Some(cached) = commit_authorship_cache.get(&hunk.commit_sha) {
            cached.clone()
        } else {
            // Try to get authorship log for this commit
            let authorship = read_authorship_v3(repo, &hunk.commit_sha).ok();
            commit_authorship_cache.insert(hunk.commit_sha.clone(), authorship.clone());
            authorship
        };

        // If we have AI authorship data, look up the author for lines in this hunk
        if let Some(ref authorship_log) = authorship_log {
            commits_with_notes.insert(hunk.commit_sha.clone());

            // Collect humans from this authorship log
            for (human_id, human_record) in &authorship_log.metadata.humans {
                humans
                    .entry(human_id.clone())
                    .or_insert_with(|| human_record.clone());
            }

            // Collect session records from this authorship log
            for (session_id, session_record) in &authorship_log.metadata.sessions {
                session_records
                    .entry(session_id.clone())
                    .or_insert_with(|| session_record.clone());
            }

            // Check each line in this hunk for AI authorship using compact schema
            // IMPORTANT: Use the original line numbers from the commit, not the current line numbers
            // Use the original filename from git blame (handles renames)
            let lookup_path = hunk.orig_filename.as_deref().unwrap_or(file_path);
            let num_lines = hunk.range.1 - hunk.range.0 + 1;
            for i in 0..num_lines {
                let current_line_num = hunk.range.0 + i;
                let orig_line_num = hunk.orig_range.0 + i;

                if let Some((author, prompt_hash, prompt)) = get_line_attribution(
                    authorship_log,
                    repo,
                    lookup_path,
                    orig_line_num,
                    &mut foreign_prompts_cache,
                ) {
                    // If this line is AI-assisted, display the tool name; otherwise the human username
                    if let Some(prompt_record) = prompt {
                        let prompt_hash = prompt_hash.unwrap();
                        // Track that this prompt hash appears in this commit
                        prompt_commits
                            .entry(prompt_hash.clone())
                            .or_default()
                            .insert(hunk.commit_sha.clone());
                        if options.use_prompt_hashes_as_names {
                            line_authors.insert(current_line_num, prompt_hash.clone());
                        } else {
                            line_authors
                                .insert(current_line_num, prompt_record.agent_id.tool.clone());
                        }

                        prompt_records.insert(prompt_hash, prompt_record.clone());
                    } else if let Some(ref hash) = prompt_hash
                        && hash.starts_with("h_")
                    {
                        // Known human attestation (h_-prefixed hash from KnownHuman checkpoint)
                        if options.use_prompt_hashes_as_names {
                            line_authors.insert(current_line_num, hash.clone());
                        } else if options.return_human_authors_as_human {
                            line_authors.insert(
                                current_line_num,
                                CheckpointKind::Human.to_str().to_string(),
                            );
                        } else {
                            line_authors.insert(current_line_num, author.username.clone());
                        }
                    } else {
                        // Has authorship log but line not AI and not KnownHuman = unattested
                        if options.return_human_authors_as_human {
                            line_authors.insert(
                                current_line_num,
                                CheckpointKind::Human.to_str().to_string(),
                            );
                        } else {
                            line_authors.insert(current_line_num, author.username.clone());
                        }
                    }
                } else {
                    // Has authorship log but no attribution found = unattested (unknown)
                    if options.return_human_authors_as_human {
                        line_authors
                            .insert(current_line_num, CheckpointKind::Human.to_str().to_string());
                    } else {
                        line_authors.insert(current_line_num, hunk.original_author.clone());
                    }
                }
            }
        } else if let Some(tool) =
            crate::operations::authorship::agent_detection::match_email_to_agent(&hunk.author_email)
        {
            // No authorship log, but commit author email matches a known AI agent.
            // Simulate authorship data so this commit is attributed to the agent.
            let (simulated_log, prompt_hash) =
                crate::operations::authorship::agent_detection::simulate_agent_authorship(
                    &hunk.commit_sha,
                    tool,
                    file_path,
                    hunk.range.0,
                    hunk.range.1,
                );

            // Merge this hunk's simulated data into a per-commit simulated log.
            // (A single agent commit can produce multiple non-contiguous blame hunks.)
            simulated_authorship_logs
                .entry(hunk.commit_sha.clone())
                .and_modify(|existing| {
                    // Merge attestation entries for this file
                    if let Some(file_attestation) = simulated_log.attestations.first() {
                        for entry in &file_attestation.entries {
                            existing
                                .get_or_create_file(file_path)
                                .add_entry(entry.clone());
                        }
                    }

                    // Merge prompt stats (sum line counts across hunks)
                    if let Some(pr) = simulated_log.metadata.prompts.get(&prompt_hash) {
                        if let Some(existing_pr) = existing.metadata.prompts.get_mut(&prompt_hash) {
                            existing_pr.total_additions += pr.total_additions;
                            existing_pr.accepted_lines += pr.accepted_lines;
                        } else {
                            existing
                                .metadata
                                .prompts
                                .insert(prompt_hash.clone(), pr.clone());
                        }
                    }
                })
                .or_insert_with(|| simulated_log.clone());

            // Insert (merged) prompt record and track commits
            if let Some(pr) = simulated_authorship_logs
                .get(&hunk.commit_sha)
                .and_then(|log| log.metadata.prompts.get(&prompt_hash))
            {
                prompt_records.insert(prompt_hash.clone(), pr.clone());
                prompt_commits
                    .entry(prompt_hash.clone())
                    .or_default()
                    .insert(hunk.commit_sha.clone());
            }

            // Mark all lines in this hunk as AI-authored by the detected tool
            for line_num in hunk.range.0..=hunk.range.1 {
                if options.use_prompt_hashes_as_names {
                    line_authors.insert(line_num, prompt_hash.clone());
                } else {
                    line_authors.insert(line_num, tool.to_string());
                }
            }
        } else {
            // No authorship log for this commit and not a known agent
            for line_num in hunk.range.0..=hunk.range.1 {
                if options.mark_unknown {
                    // User wants explicit distinction - mark as Unknown
                    line_authors.insert(line_num, "Unknown".to_string());
                } else if options.return_human_authors_as_human {
                    line_authors.insert(line_num, CheckpointKind::Human.to_str().to_string());
                } else {
                    line_authors.insert(line_num, hunk.original_author.clone());
                }
            }
        }
    }

    // Collect all authorship logs we've seen (for JSON output to find other files)
    let mut authorship_logs: Vec<AuthorshipLog> =
        commit_authorship_cache.into_values().flatten().collect();
    authorship_logs.extend(simulated_authorship_logs.into_values());

    // Convert HashSet to Vec and sort for deterministic output
    let prompt_commits_vec: HashMap<String, Vec<String>> = prompt_commits
        .into_iter()
        .map(|(hash, commits)| {
            let mut commits_vec: Vec<String> = commits.into_iter().collect();
            commits_vec.sort();
            (hash, commits_vec)
        })
        .collect();

    Ok((
        line_authors,
        prompt_records,
        session_records,
        humans,
        authorship_logs,
        prompt_commits_vec,
        commits_with_notes,
    ))
}
