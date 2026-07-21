use super::types::VirtualAttributions;
use crate::error::GitAiError;
use crate::model::authorship_log::{HumanRecord, SessionRecord};
use crate::model::working_log::CheckpointKind;
use crate::operations::authorship::attribution_tracker::{
    LineAttribution, attributions_to_line_attributions, line_attributions_to_attributions,
};
use crate::operations::git::repository::Repository;
use std::collections::{BTreeMap, HashMap, HashSet};

impl VirtualAttributions {
    /// Create VirtualAttributions from only the persisted working-log state.
    ///
    /// Unlike `from_just_working_log`, this never reads the live worktree. It is intended for
    /// daemon-side async reconstruction where the command's final state has already been captured.
    pub fn from_persisted_working_log(
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
            let file_content = working_log
                .stored_initial_file_content_from(&initial_attributions, file_path)
                .ok_or_else(|| {
                    GitAiError::Generic(format!(
                        "INITIAL missing persisted file snapshot for {}",
                        file_path
                    ))
                })?;
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

                let file_content = working_log.get_file_version(&entry.blob_sha)?;
                file_contents.insert(entry.file.clone(), file_content.clone());

                let line_attrs = if entry.line_attributions.is_empty() {
                    attributions_to_line_attributions(&entry.attributions, &file_content)
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

    /// Build amend attributions from the original commit's blame data, persisted
    /// working-log checkpoints, and an explicit final-state snapshot.
    pub async fn from_working_log_for_commit_snapshot(
        repo: Repository,
        base_commit: String,
        pathspecs: &[String],
        human_author: Option<String>,
        blame_start_commit: Option<String>,
        final_state_snapshot: &HashMap<String, String>,
    ) -> Result<Self, GitAiError> {
        let blame_va = Self::new_for_base_commit(
            repo.clone(),
            base_commit.clone(),
            pathspecs,
            blame_start_commit,
        )
        .await?;

        let checkpoint_va =
            Self::from_persisted_working_log(repo.clone(), base_commit.clone(), human_author)?;

        // Save session prompt IDs before the merge consumes checkpoint_va.
        // Exclude INITIAL-only prompts from prior commits.
        let checkpoint_prompt_ids: std::collections::HashSet<String> = checkpoint_va
            .prompts
            .keys()
            .filter(|id| !checkpoint_va.initial_only_prompt_ids.contains(*id))
            .cloned()
            .collect();

        let final_state = final_state_snapshot.clone();
        let mut merged_va =
            crate::operations::authorship::virtual_attribution::merge_attributions_favoring_first(
                checkpoint_va,
                blame_va,
                final_state,
            )?;

        // Mark all non-session prompts (same logic as `from_working_log_for_commit`).
        merged_va.initial_only_prompt_ids = merged_va
            .prompts
            .keys()
            .filter(|id| !checkpoint_prompt_ids.contains(*id))
            .cloned()
            .collect();

        // Prune blame-history prompts whose lines were deleted.  Same logic as
        // `from_working_log_for_commit`.
        let referenced_in_merged: std::collections::HashSet<String> = merged_va
            .attributions
            .values()
            .flat_map(|(_, line_attrs)| line_attrs.iter())
            .map(|la| la.author_id.clone())
            .collect();
        merged_va.prompts.retain(|id, _| {
            checkpoint_prompt_ids.contains(id) || referenced_in_merged.contains(id)
        });
        merged_va
            .humans
            .retain(|id, _| referenced_in_merged.contains(id));
        let referenced_session_ids: std::collections::HashSet<String> = referenced_in_merged
            .iter()
            .filter(|id| id.starts_with("s_"))
            .map(|id| id.split("::").next().unwrap_or(id).to_string())
            .collect();
        merged_va
            .sessions
            .retain(|id, _| referenced_session_ids.contains(id));

        Ok(merged_va)
    }
}
