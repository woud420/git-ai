use super::types::VirtualAttributions;
use crate::error::GitAiError;
use crate::model::authorship_log::{HumanRecord, SessionRecord};
use crate::model::working_log::CheckpointKind;
use crate::operations::authorship::attribution_tracker::{
    LineAttribution, line_attributions_to_attributions,
};
use crate::operations::git::repository::Repository;
use std::collections::{BTreeMap, HashMap, HashSet};

impl VirtualAttributions {
    /// Create VirtualAttributions from just the working log (no blame)
    ///
    /// This is a fast path that skips the expensive blame operation.
    /// Use this when you only care about working log data and don't need historical blame.
    ///
    /// This function:
    /// 1. Loads INITIAL attributions (unstaged AI code from previous working state)
    /// 2. Applies working log checkpoints on top
    /// 3. Returns VirtualAttributions with just the working log data
    pub fn from_just_working_log(
        repo: Repository,
        base_commit: String,
        human_author: Option<String>,
    ) -> Result<Self, GitAiError> {
        let working_log = repo.storage.working_log_for_base_commit(&base_commit)?;
        let initial_attributions = working_log.read_initial_attributions();
        let checkpoints = working_log.read_all_checkpoints().unwrap_or_default();

        let mut attributions: HashMap<
            String,
            (
                Vec<crate::operations::authorship::attribution_tracker::Attribution>,
                Vec<LineAttribution>,
            ),
        > = HashMap::new();
        let mut prompts = BTreeMap::new();
        let mut humans: BTreeMap<String, HumanRecord> = BTreeMap::new();
        let mut file_contents: HashMap<String, String> = HashMap::new();
        // Prompt IDs that originate from INITIAL attributions (prior commits).
        // If a checkpoint later references the same prompt_id, it is removed from
        // this set because the prompt was actively used in this commit's session.
        let mut initial_only_prompt_ids: HashSet<String> = HashSet::new();
        let mut sessions: BTreeMap<String, SessionRecord> = BTreeMap::new();

        // Track additions and deletions per session_id for metrics
        let mut session_additions: HashMap<String, u32> = HashMap::new();
        let mut session_deletions: HashMap<String, u32> = HashMap::new();

        // Add prompts from INITIAL attributions
        // These are uncommitted prompts, so we use an empty string as the commit_sha
        for (prompt_id, prompt_record) in &initial_attributions.prompts {
            prompts
                .entry(prompt_id.clone())
                .or_insert_with(BTreeMap::new)
                .insert(String::new(), prompt_record.clone());
            initial_only_prompt_ids.insert(prompt_id.clone());
        }

        // Load known human records from INITIAL attributions
        for (hash, human_record) in &initial_attributions.humans {
            humans
                .entry(hash.clone())
                .or_insert_with(|| human_record.clone());
        }

        // Load session records from INITIAL attributions
        for (session_id, session_record) in &initial_attributions.sessions {
            sessions
                .entry(session_id.clone())
                .or_insert_with(|| session_record.clone());
        }

        // Process INITIAL attributions
        for (file_path, line_attrs) in &initial_attributions.files {
            // Get the latest file content from working directory
            if let Ok(workdir) = repo.workdir() {
                let abs_path = workdir.join(file_path);
                let file_content = if abs_path.exists() {
                    std::fs::read_to_string(&abs_path).unwrap_or_default()
                } else {
                    String::new()
                };
                file_contents.insert(file_path.clone(), file_content.clone());

                // Convert line attributions to character attributions
                let char_attrs = line_attributions_to_attributions(line_attrs, &file_content, 0);
                attributions.insert(file_path.clone(), (char_attrs, line_attrs.clone()));
            }
        }

        // Collect attributions from all checkpoints (later checkpoints override earlier ones)
        for checkpoint in &checkpoints {
            // Add prompts or sessions from checkpoint
            if let Some(agent_id) = &checkpoint.agent_id {
                let is_session_format = checkpoint.trace_id.is_some();

                if is_session_format {
                    // New format: derive session_id from this checkpoint's own agent_id
                    let session_id =
                        crate::model::authorship_log_serialization::generate_session_id(
                            &agent_id.id,
                            &agent_id.tool,
                        );

                    let session_record = SessionRecord {
                        agent_id: agent_id.clone(),
                        human_author: human_author.clone(),
                        custom_attributes: None,
                    };

                    sessions.insert(session_id.clone(), session_record);

                    // Track additions/deletions keyed by session_id
                    *session_additions.entry(session_id.clone()).or_insert(0) +=
                        checkpoint.line_stats.additions;
                    *session_deletions.entry(session_id).or_insert(0) +=
                        checkpoint.line_stats.deletions;
                } else {
                    // Old format: use existing prompts logic
                    let author_id = crate::model::authorship_log_serialization::generate_short_hash(
                        &agent_id.id,
                        &agent_id.tool,
                    );
                    // For working log checkpoints, use empty string as commit_sha since they're uncommitted
                    // Always overwrite with the latest checkpoint for this agent so refreshed
                    // transcripts/models from post-commit aren't lost.
                    let prompt_record = crate::model::authorship_log::PromptRecord {
                        agent_id: agent_id.clone(),
                        human_author: human_author.clone(),
                        total_additions: 0,
                        total_deletions: 0,
                        accepted_lines: 0,
                        overriden_lines: 0,
                        custom_attributes: None,
                        messages_url: None,
                    };

                    prompts
                        .entry(author_id.clone())
                        .or_insert_with(BTreeMap::new)
                        .insert(String::new(), prompt_record);
                    // This prompt was actively used in a checkpoint, so it's not
                    // INITIAL-only (even if it was also in INITIAL).
                    initial_only_prompt_ids.remove(&author_id);

                    // Track additions and deletions from checkpoint line_stats
                    *session_additions.entry(author_id.clone()).or_insert(0) +=
                        checkpoint.line_stats.additions;
                    *session_deletions.entry(author_id).or_insert(0) +=
                        checkpoint.line_stats.deletions;
                }
            }

            if checkpoint.kind == CheckpointKind::KnownHuman {
                let hash = crate::model::authorship_log_serialization::generate_human_short_hash(
                    &checkpoint.author,
                );
                humans.entry(hash).or_insert_with(|| HumanRecord {
                    author: checkpoint.author.clone(),
                });
            }

            // Collect attributions from checkpoint entries
            for entry in &checkpoint.entries {
                // Most human-only pre-commit entries carry no attribution data and can be skipped.
                // This keeps post-commit work proportional to AI-relevant files.
                if entry.line_attributions.is_empty() && entry.attributions.is_empty() {
                    continue;
                }

                // Get the latest file content from working directory
                if let Ok(workdir) = repo.workdir() {
                    let abs_path = workdir.join(&entry.file);
                    let file_content = if abs_path.exists() {
                        std::fs::read_to_string(&abs_path).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    file_contents.insert(entry.file.clone(), file_content);
                }

                // Prefer persisted line attributions. Fall back to converting char attributions
                // for compatibility with older checkpoint data.
                let file_content = file_contents.get(&entry.file).cloned().unwrap_or_default();
                let line_attrs = if entry.line_attributions.is_empty() {
                    crate::operations::authorship::attribution_tracker::attributions_to_line_attributions(
                        &entry.attributions,
                        &file_content,
                    )
                } else {
                    entry.line_attributions.clone()
                };

                if line_attrs.is_empty() {
                    // The entry had attribution data but no AI lines remain after
                    // filtering (e.g. human rewrote the entire file).  Clear any
                    // stale AI attributions from earlier checkpoints for this file.
                    attributions.remove(&entry.file);
                    continue;
                }

                let char_attrs = line_attributions_to_attributions(&line_attrs, &file_content, 0);

                attributions.insert(entry.file.clone(), (char_attrs, line_attrs));
            }
        }

        // Calculate final metrics for each prompt
        Self::calculate_and_update_prompt_metrics(
            &mut prompts,
            &attributions,
            &session_additions,
            &session_deletions,
        );

        Ok(VirtualAttributions {
            repo,
            base_commit,
            attributions,
            file_contents,
            prompts,
            ts: 0,
            blame_start_commit: None,
            humans,
            initial_only_prompt_ids,
            sessions,
        })
    }

    /// Create VirtualAttributions from working-log state using an exact captured snapshot
    /// instead of the live worktree.
    pub fn from_working_log_snapshot(
        repo: Repository,
        base_commit: String,
        human_author: Option<String>,
        final_state_snapshot: &HashMap<String, String>,
    ) -> Result<Self, GitAiError> {
        let working_log = repo.storage.working_log_for_base_commit(&base_commit)?;
        let initial_attributions = working_log.read_initial_attributions();
        let checkpoints = working_log.read_all_checkpoints().unwrap_or_default();

        let mut attributions: HashMap<
            String,
            (
                Vec<crate::operations::authorship::attribution_tracker::Attribution>,
                Vec<LineAttribution>,
            ),
        > = HashMap::new();
        let mut prompts = BTreeMap::new();
        let mut humans: BTreeMap<String, HumanRecord> = BTreeMap::new();
        let mut file_contents: HashMap<String, String> = HashMap::new();
        let mut initial_only_prompt_ids: HashSet<String> = HashSet::new();
        let mut sessions: BTreeMap<String, SessionRecord> = BTreeMap::new();

        let mut session_additions: HashMap<String, u32> = HashMap::new();
        let mut session_deletions: HashMap<String, u32> = HashMap::new();

        for (prompt_id, prompt_record) in &initial_attributions.prompts {
            prompts
                .entry(prompt_id.clone())
                .or_insert_with(BTreeMap::new)
                .insert(String::new(), prompt_record.clone());
            initial_only_prompt_ids.insert(prompt_id.clone());
        }

        // Load known human records from INITIAL attributions
        for (hash, human_record) in &initial_attributions.humans {
            humans
                .entry(hash.clone())
                .or_insert_with(|| human_record.clone());
        }

        // Load session records from INITIAL attributions
        for (session_id, session_record) in &initial_attributions.sessions {
            sessions
                .entry(session_id.clone())
                .or_insert_with(|| session_record.clone());
        }

        for (file_path, line_attrs) in &initial_attributions.files {
            // Use stored content for INITIAL since line_attrs reference that file version.
            // Fall back to final_state_snapshot only if no stored content exists.
            let file_content = working_log
                .stored_initial_file_content_from(&initial_attributions, file_path)
                .or_else(|| final_state_snapshot.get(file_path).cloned())
                .unwrap_or_default();
            file_contents.insert(file_path.clone(), file_content.clone());

            let char_attrs = line_attributions_to_attributions(line_attrs, &file_content, 0);
            attributions.insert(file_path.clone(), (char_attrs, line_attrs.clone()));
        }

        for checkpoint in &checkpoints {
            if let Some(agent_id) = &checkpoint.agent_id {
                let is_session_format = checkpoint.trace_id.is_some();

                if is_session_format {
                    // New format: derive session_id from this checkpoint's own agent_id
                    let session_id =
                        crate::model::authorship_log_serialization::generate_session_id(
                            &agent_id.id,
                            &agent_id.tool,
                        );

                    let session_record = SessionRecord {
                        agent_id: agent_id.clone(),
                        human_author: human_author.clone(),
                        custom_attributes: None,
                    };

                    sessions.insert(session_id.clone(), session_record);

                    // Track additions/deletions keyed by session_id
                    *session_additions.entry(session_id.clone()).or_insert(0) +=
                        checkpoint.line_stats.additions;
                    *session_deletions.entry(session_id).or_insert(0) +=
                        checkpoint.line_stats.deletions;
                } else {
                    // Old format: use existing prompts logic
                    let author_id = crate::model::authorship_log_serialization::generate_short_hash(
                        &agent_id.id,
                        &agent_id.tool,
                    );
                    let prompt_record = crate::model::authorship_log::PromptRecord {
                        agent_id: agent_id.clone(),
                        human_author: human_author.clone(),

                        total_additions: 0,
                        total_deletions: 0,
                        accepted_lines: 0,
                        overriden_lines: 0,

                        custom_attributes: None,
                        messages_url: None,
                    };

                    prompts
                        .entry(author_id.clone())
                        .or_insert_with(BTreeMap::new)
                        .insert(String::new(), prompt_record);
                    initial_only_prompt_ids.remove(&author_id);

                    *session_additions.entry(author_id.clone()).or_insert(0) +=
                        checkpoint.line_stats.additions;
                    *session_deletions.entry(author_id.clone()).or_insert(0) +=
                        checkpoint.line_stats.deletions;
                }
            }

            if checkpoint.kind == CheckpointKind::KnownHuman {
                let hash = crate::model::authorship_log_serialization::generate_human_short_hash(
                    &checkpoint.author,
                );
                humans.entry(hash).or_insert_with(|| HumanRecord {
                    author: checkpoint.author.clone(),
                });
            }

            for entry in &checkpoint.entries {
                if entry.line_attributions.is_empty() && entry.attributions.is_empty() {
                    continue;
                }

                let file_content = final_state_snapshot
                    .get(&entry.file)
                    .cloned()
                    .unwrap_or_else(|| {
                        working_log
                            .get_file_version(&entry.blob_sha)
                            .unwrap_or_default()
                    });
                file_contents.insert(entry.file.clone(), file_content.clone());

                let line_attrs = if entry.line_attributions.is_empty() {
                    crate::operations::authorship::attribution_tracker::attributions_to_line_attributions(
                        &entry.attributions,
                        &file_content,
                    )
                } else {
                    entry.line_attributions.clone()
                };

                if line_attrs.is_empty() {
                    // The entry had attribution data but no AI lines remain after
                    // filtering (e.g. human rewrote the entire file).  Clear any
                    // stale AI attributions from earlier checkpoints for this file.
                    attributions.remove(&entry.file);
                    continue;
                }

                let char_attrs = line_attributions_to_attributions(&line_attrs, &file_content, 0);
                attributions.insert(entry.file.clone(), (char_attrs, line_attrs));
            }
        }

        Self::calculate_and_update_prompt_metrics(
            &mut prompts,
            &attributions,
            &session_additions,
            &session_deletions,
        );

        Ok(VirtualAttributions {
            repo,
            base_commit,
            attributions,
            file_contents,
            prompts,
            ts: 0,
            blame_start_commit: None,
            humans,
            initial_only_prompt_ids,
            sessions,
        })
    }
}
