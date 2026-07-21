use super::carryover_merge::{checkout_merge_rebased_content, merged_carryover_content_pure};
use super::diff_utils::batch_file_contents;
use crate::error::GitAiError;
use crate::operations::git::repository::Repository;
use std::collections::{HashMap, HashSet};

pub fn checkout_merge_final_state_snapshot(
    repo: &Repository,
    old_head: &str,
    new_head: &str,
) -> Result<HashMap<String, String>, GitAiError> {
    if old_head.is_empty() || new_head.is_empty() || old_head == new_head {
        return Ok(HashMap::new());
    }
    if !repo.storage.has_working_log(old_head) {
        return Ok(HashMap::new());
    }

    let working_log = repo.storage.working_log_for_base_commit(old_head)?;
    let observed_snapshot = working_log.observed_file_snapshot()?;

    // Batch-read base (old_head) + target (new_head) content for every observed
    // file in two git spawns instead of two spawns PER file.
    let mut requests: Vec<(String, String)> = Vec::with_capacity(observed_snapshot.len() * 2);
    for file_path in observed_snapshot.keys() {
        requests.push((old_head.to_string(), file_path.clone()));
        requests.push((new_head.to_string(), file_path.clone()));
    }
    let contents = batch_file_contents(repo, &requests)?;

    let mut final_state = HashMap::new();
    for (file_path, observed_content) in observed_snapshot {
        let base_content = contents
            .get(&(old_head.to_string(), file_path.clone()))
            .cloned()
            .unwrap_or_default();
        let target_content = contents
            .get(&(new_head.to_string(), file_path.clone()))
            .cloned()
            .unwrap_or_default();
        let content =
            checkout_merge_rebased_content(&base_content, &target_content, &observed_content);
        final_state.insert(file_path, content);
    }
    Ok(final_state)
}

pub(super) fn build_carryover_snapshot(
    repo: &Repository,
    parent_sha: &str,
    commit_sha: &str,
    pathspecs: Option<&HashSet<String>>,
    observed_snapshot: &HashMap<String, String>,
) -> Result<HashMap<String, String>, GitAiError> {
    let file_paths: HashSet<String> = match pathspecs {
        Some(paths) => paths.iter().cloned().collect(),
        None => observed_snapshot.keys().cloned().collect(),
    };

    // Batch-read committed (commit_sha) content for every file, plus parent
    // (parent_sha) content for the files we may need to 3-way reconcile. Two
    // git spawns total instead of up to ~2 per file.
    let mut requests: Vec<(String, String)> = Vec::new();
    for file_path in &file_paths {
        requests.push((commit_sha.to_string(), file_path.clone()));
        if parent_sha != "initial" && observed_snapshot.contains_key(file_path) {
            requests.push((parent_sha.to_string(), file_path.clone()));
        }
    }
    let contents = batch_file_contents(repo, &requests)?;

    let mut carryover_snapshot = HashMap::new();
    for file_path in file_paths {
        let committed_content = contents
            .get(&(commit_sha.to_string(), file_path.clone()))
            .cloned()
            .unwrap_or_default();
        let content = if let Some(observed_content) = observed_snapshot.get(&file_path) {
            let parent_content = if parent_sha == "initial" {
                String::new()
            } else {
                contents
                    .get(&(parent_sha.to_string(), file_path.clone()))
                    .cloned()
                    .unwrap_or_default()
            };
            merged_carryover_content_pure(&parent_content, &committed_content, observed_content)
        } else {
            committed_content
        };
        carryover_snapshot.insert(file_path, content);
    }

    Ok(carryover_snapshot)
}
