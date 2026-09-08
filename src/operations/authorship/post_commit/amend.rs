use super::committed_hunks::recovery_committed_hunks;
use crate::config::Config;
use crate::error::GitAiError;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::authorship::attribution_recovery::{
    AttributionRecoveryContext, FileTimestampsByPath, UnknownLinesByFile,
};
use crate::operations::authorship::recovery_stores::RecoveryStores;
use crate::operations::authorship::virtual_attribution::VirtualAttributions;
use crate::operations::git::notes_api::write_note;
use crate::operations::git::repository::{Repository, batch_read_paths_at_treeishes};
use std::collections::{HashMap, HashSet};

fn commit_tree_snapshot_for_files(
    repo: &Repository,
    commit_sha: &str,
    file_paths: &HashSet<String>,
) -> Result<HashMap<String, String>, GitAiError> {
    let requests = file_paths
        .iter()
        .map(|file_path| (commit_sha.to_string(), file_path.clone()))
        .collect::<Vec<_>>();
    let contents = batch_read_paths_at_treeishes(repo, &requests)?;
    let mut snapshot = HashMap::with_capacity(file_paths.len());
    for file_path in file_paths {
        snapshot.insert(
            file_path.clone(),
            contents
                .get(&(commit_sha.to_string(), file_path.clone()))
                .cloned()
                .unwrap_or_default(),
        );
    }

    Ok(snapshot)
}

/// Amend-specific post-commit that merges blame-sourced attributions from the
/// original commit with persisted working-log checkpoint data.
pub fn post_commit_amend(
    repo: &Repository,
    original_commit: &str,
    amended_commit: &str,
    human_author: String,
) -> Result<(String, AuthorshipLog), GitAiError> {
    post_commit_amend_with_recovery_timestamps(
        repo,
        original_commit,
        amended_commit,
        human_author,
        None,
        None,
    )
}

pub(crate) struct PostCommitAmendResult {
    pub commit_sha: String,
    pub authorship_log: AuthorshipLog,
    pub authorship_note: String,
    pub parent_sha: String,
}

pub(crate) fn post_commit_amend_with_recovery_timestamps(
    repo: &Repository,
    original_commit: &str,
    amended_commit: &str,
    human_author: String,
    recovery_file_timestamps: Option<&FileTimestampsByPath>,
    before_external_recovery: Option<&dyn Fn(&UnknownLinesByFile)>,
) -> Result<(String, AuthorshipLog), GitAiError> {
    post_commit_amend_with_recovery_timestamps_detailed(
        repo,
        original_commit,
        amended_commit,
        human_author,
        recovery_file_timestamps,
        before_external_recovery,
    )
    .map(|result| (result.commit_sha, result.authorship_log))
}

pub(crate) fn post_commit_amend_with_recovery_timestamps_detailed(
    repo: &Repository,
    original_commit: &str,
    amended_commit: &str,
    human_author: String,
    recovery_file_timestamps: Option<&FileTimestampsByPath>,
    before_external_recovery: Option<&dyn Fn(&UnknownLinesByFile)>,
) -> Result<PostCommitAmendResult, GitAiError> {
    let repo_storage = &repo.storage;
    let working_log = repo_storage.working_log_for_base_commit(original_commit)?;

    // Compute pathspecs: changed files in the amended commit + working log touched files
    let changed_files = repo.list_commit_files(amended_commit, None)?;
    let mut pathspecs: HashSet<String> = changed_files.into_iter().collect();
    let touched_files = working_log.all_touched_files()?;
    pathspecs.extend(touched_files);
    let initial_attributions_for_pathspecs = working_log.read_initial_attributions();
    for file_path in initial_attributions_for_pathspecs.files.keys() {
        pathspecs.insert(file_path.clone());
    }
    let pathspecs_vec: Vec<String> = pathspecs.iter().cloned().collect();
    let observed_snapshot = working_log.observed_file_snapshot()?;
    let mut final_state_snapshot =
        commit_tree_snapshot_for_files(repo, amended_commit, &pathspecs)?;
    final_state_snapshot.extend(observed_snapshot);

    // Check if original commit has existing authorship data. Read through
    // notes_api so the HTTP notes backend is honored (the note may only exist
    // in the notes-db cache, not refs/notes/ai).
    let has_existing_data =
        crate::operations::git::notes_api::read_authorship_v3(repo, original_commit)
            .map(|log| {
                !log.metadata.prompts.is_empty()
                    || !log.metadata.humans.is_empty()
                    || !log.metadata.sessions.is_empty()
            })
            .unwrap_or(false);

    let working_va = crate::tokio_runtime::block_on(async {
        VirtualAttributions::from_working_log_for_commit_snapshot(
            repo.clone(),
            original_commit.to_string(),
            &pathspecs_vec,
            if has_existing_data {
                None
            } else {
                Some(human_author.clone())
            },
            None,
            &final_state_snapshot,
        )
        .await
    })?;

    // Resolve parent of the amended commit for diff base
    let amended_commit_obj = repo.find_commit(amended_commit.to_string())?;
    let parent_sha = if amended_commit_obj.parent_count()? > 0 {
        amended_commit_obj
            .parent(0)
            .map(|p| p.id())
            .unwrap_or_else(|_| "initial".to_string())
    } else {
        "initial".to_string()
    };

    let (mut authorship_log, initial_attributions, initial_file_contents) = working_va
        .to_authorship_log_and_initial_working_log(
            repo,
            &parent_sha,
            amended_commit,
            Some(&pathspecs),
            Some(&final_state_snapshot),
        )?;

    authorship_log.metadata.base_commit_sha = amended_commit.to_string();

    // Fill unattributed lines for background agents
    if !matches!(
        crate::operations::authorship::background_agent::detect(),
        crate::operations::authorship::background_agent::BackgroundAgent::None
            | crate::operations::authorship::background_agent::BackgroundAgent::WithHooks { .. }
    ) {
        let diff_base = if parent_sha == "initial" {
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        } else {
            &parent_sha
        };
        if let Ok(added_lines) = repo.diff_added_lines(diff_base, amended_commit, None) {
            let committed_hunks: HashMap<String, Vec<crate::model::authorship_log::LineRange>> =
                added_lines
                    .into_iter()
                    .filter(|(_, lines)| !lines.is_empty())
                    .map(|(path, lines)| {
                        (
                            path,
                            crate::model::authorship_log::LineRange::compress_lines(&lines),
                        )
                    })
                    .collect();
            crate::operations::authorship::background_agent::fill_unattributed_lines(
                &mut authorship_log,
                &committed_hunks,
                &human_author,
            );
        }
    }

    let recovery_hunks = recovery_committed_hunks(repo, &parent_sha, amended_commit, None)?;
    crate::operations::authorship::attribution_recovery::recover_attribution(
        repo,
        &parent_sha,
        amended_commit,
        &human_author,
        &mut authorship_log,
        &recovery_hunks,
        AttributionRecoveryContext {
            file_timestamps: recovery_file_timestamps,
            before_external_recovery,
            stores: RecoveryStores::resolve(),
        },
    )?;
    authorship_log.metadata.base_commit_sha = amended_commit.to_string();

    // Preserve human/session metadata from the original commit's note. Read
    // through notes_api so the HTTP notes backend is honored — with refs-only
    // reads the amended note keeps its s_/h_ attestation hashes but silently
    // loses the sessions/humans records they resolve through.
    if let Ok(original_log) =
        crate::operations::git::notes_api::read_authorship_v3(repo, original_commit)
    {
        for (id, record) in original_log.metadata.humans {
            authorship_log.metadata.humans.entry(id).or_insert(record);
        }
        let referenced_session_ids: HashSet<String> = authorship_log
            .attestations
            .iter()
            .flat_map(|fa| fa.entries.iter())
            .filter_map(|entry| {
                if entry.hash.starts_with("s_") {
                    Some(
                        entry
                            .hash
                            .split("::")
                            .next()
                            .unwrap_or(&entry.hash)
                            .to_string(),
                    )
                } else {
                    None
                }
            })
            .collect();
        for (id, record) in original_log.metadata.sessions {
            if referenced_session_ids.contains(&id) {
                authorship_log.metadata.sessions.entry(id).or_insert(record);
            }
        }
    }

    // Inject custom attributes
    let custom_attrs = Config::fresh().custom_attributes().clone();
    if !custom_attrs.is_empty() {
        for pr in authorship_log.metadata.prompts.values_mut() {
            pr.custom_attributes = Some(custom_attrs.clone());
        }
        for sr in authorship_log.metadata.sessions.values_mut() {
            sr.custom_attributes = Some(custom_attrs.clone());
        }
    }

    let authorship_note_str = authorship_log
        .serialize_to_string()
        .map_err(|_| GitAiError::Generic("Failed to serialize authorship log".to_string()))?;
    write_note(repo, amended_commit, &authorship_note_str)?;

    // Write INITIAL file for uncommitted attributions
    if !initial_attributions.files.is_empty() {
        let new_working_log = repo_storage.working_log_for_base_commit(amended_commit)?;
        new_working_log.write_initial_attributions_with_contents(
            initial_attributions.files,
            initial_attributions.prompts,
            initial_attributions.humans,
            initial_file_contents,
            initial_attributions.sessions,
        )?;
    }

    // Clean up old working log
    repo_storage.delete_working_log_for_base_commit(original_commit)?;

    Ok(PostCommitAmendResult {
        commit_sha: amended_commit.to_string(),
        authorship_log,
        authorship_note: authorship_note_str,
        parent_sha,
    })
}
