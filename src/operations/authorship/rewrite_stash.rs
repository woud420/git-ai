mod partition;
mod storage_paths;
use storage_paths::{
    cleanup_legacy_stashes_dir, stash_entry_dir, stash_metadata_path, working_log_for_dir,
};

mod reconstruct;
use reconstruct::reconstruct_stash_applied_contents;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;

use crate::error::GitAiError;
use crate::model::attribution_tracker::LineAttribution;
use crate::model::authorship_log::{HumanRecord, PromptRecord, SessionRecord};
use crate::model::imara_diff_utils::{DiffOp, capture_diff_slices};
use crate::model::repository::error::PersistenceError;
use crate::model::working_log::{CheckpointKind, InitialAttributions};
use crate::operations::git::repo_storage::PersistedWorkingLog;
use crate::operations::git::repository::Repository;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StashMetadata {
    pub base_commit: String,
    pub timestamp: u64,
    #[serde(default)]
    pub pathspecs: Vec<String>,
}

fn path_matches_any(path: &str, pathspecs: &[String]) -> bool {
    pathspecs.iter().any(|spec| {
        // Trailing-`*` prefix glob (e.g. `src/foo*`, or a bare `*`), matching
        // the pathspec semantics the pre-rewrite stash matcher supported.
        if let Some(prefix) = spec.strip_suffix('*') {
            return path.starts_with(prefix);
        }
        let normalized = spec.trim_end_matches('/');
        path == spec || path == normalized || {
            let prefix = format!("{}/", normalized);
            path.starts_with(&prefix)
        }
    })
}

pub fn handle_stash_create(
    repo: &Repository,
    stash_sha: &str,
    head_sha: &str,
    pathspecs: Vec<String>,
    keep_index: bool,
) -> Result<(), GitAiError> {
    cleanup_legacy_stashes_dir(repo);

    let metadata = StashMetadata {
        base_commit: head_sha.to_string(),
        timestamp: crate::model::clock::now_secs(),
        pathspecs: pathspecs.clone(),
    };

    let stash_dir = stash_entry_dir(repo, stash_sha);
    fs::create_dir_all(&stash_dir)?;

    let metadata_path = stash_metadata_path(repo, stash_sha);
    let json = serde_json::to_string_pretty(&metadata)?;
    fs::write(&metadata_path, json)?;

    partition::partition_stash_attributions(repo, stash_sha, head_sha, &pathspecs, keep_index)?;

    Ok(())
}

pub fn handle_stash_pop_or_apply_with_head(
    repo: &Repository,
    stash_sha: &str,
    is_pop: bool,
    target_head: Option<&str>,
) -> Result<(), GitAiError> {
    cleanup_legacy_stashes_dir(repo);

    let metadata_path = stash_metadata_path(repo, stash_sha);

    if !metadata_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&metadata_path)?;
    let metadata: StashMetadata = serde_json::from_str(&content)?;

    let Some(current_head) = target_head.filter(|h| !h.is_empty()) else {
        return Ok(());
    };

    if metadata.base_commit != current_head {
        restore_stash_attributions_with_shift(repo, stash_sha, current_head)?;
    } else {
        restore_stash_attributions(repo, stash_sha, current_head)?;
    }

    if is_pop {
        let _ = fs::remove_dir_all(stash_entry_dir(repo, stash_sha));
    }

    Ok(())
}

pub fn handle_stash_drop(repo: &Repository, stash_sha: &str) -> Result<(), GitAiError> {
    cleanup_legacy_stashes_dir(repo);
    let _ = fs::remove_dir_all(stash_entry_dir(repo, stash_sha));
    Ok(())
}

fn trim_initial_metadata_to_referenced_authors(initial: &mut InitialAttributions) {
    let human_sentinel = CheckpointKind::Human.to_str();
    let mut referenced_authors = HashSet::new();
    let mut referenced_sessions = HashSet::new();

    for attrs in initial.files.values() {
        for attr in attrs {
            if attr.author_id == human_sentinel {
                continue;
            }

            referenced_authors.insert(attr.author_id.clone());
            if attr.author_id.starts_with("s_") {
                let session_key = attr
                    .author_id
                    .split("::")
                    .next()
                    .unwrap_or(&attr.author_id)
                    .to_string();
                referenced_sessions.insert(session_key);
            }
        }
    }

    initial
        .prompts
        .retain(|author_id, _| referenced_authors.contains(author_id));
    initial
        .humans
        .retain(|author_id, _| referenced_authors.contains(author_id));
    initial
        .sessions
        .retain(|session_id, _| referenced_sessions.contains(session_id));
}

fn restore_stash_attributions(
    repo: &Repository,
    stash_sha: &str,
    current_head: &str,
) -> Result<(), GitAiError> {
    let stash_log = working_log_for_dir(repo, stash_entry_dir(repo, stash_sha), current_head);
    if !stash_log.initial_file.exists() {
        return Ok(());
    }

    let initial = stash_log.read_initial_attributions();
    if initial.files.is_empty() {
        return Ok(());
    }

    let working_log = repo.storage.working_log_for_base_commit(current_head)?;
    copy_initial_blobs(&stash_log, &working_log, &initial)?;
    remove_checkpoint_entries_for_files(&working_log, initial.files.keys().cloned())?;
    merge_initial_replacing_paths(&working_log, initial, None)?;
    Ok(())
}

fn restore_stash_attributions_with_shift(
    repo: &Repository,
    stash_sha: &str,
    current_head: &str,
) -> Result<(), GitAiError> {
    let stash_log = working_log_for_dir(repo, stash_entry_dir(repo, stash_sha), current_head);
    if !stash_log.initial_file.exists() {
        return Ok(());
    }

    let initial = stash_log.read_initial_attributions();
    if initial.files.is_empty() {
        return Ok(());
    }

    let mut stash_file_contents: HashMap<String, String> = HashMap::new();
    for file_path in initial.files.keys() {
        if let Some(content) = stash_log.stored_initial_file_content_from(&initial, file_path) {
            stash_file_contents.insert(file_path.clone(), content);
        }
    }

    // Reconstruct the applied content from immutable trees.
    let mut files: HashMap<String, Vec<LineAttribution>> = HashMap::new();
    let mut file_contents: HashMap<String, String> = HashMap::new();

    let applied_paths: Vec<String> = initial.files.keys().cloned().collect();
    let applied_contents =
        reconstruct_stash_applied_contents(repo, stash_sha, current_head, &applied_paths)?;

    for (file_path, attrs) in &initial.files {
        let stash_content = stash_file_contents
            .get(file_path)
            .cloned()
            .unwrap_or_default();
        let current_content = applied_contents.get(file_path).cloned().unwrap_or_default();

        if current_content.is_empty() {
            continue;
        }

        if stash_content == current_content {
            files.insert(file_path.clone(), attrs.clone());
            file_contents.insert(file_path.clone(), current_content);
            continue;
        }

        // Content-based shift using Equal regions
        let old_lines: Vec<&str> = stash_content.lines().collect();
        let new_lines: Vec<&str> = current_content.lines().collect();
        let ops = capture_diff_slices(&old_lines, &new_lines);

        let mut line_map: HashMap<u32, u32> = HashMap::new();
        for op in &ops {
            if let DiffOp::Equal {
                old_index,
                new_index,
                len,
            } = op
            {
                for i in 0..*len {
                    line_map.insert((*old_index + i + 1) as u32, (*new_index + i + 1) as u32);
                }
            }
        }

        let shifted: Vec<LineAttribution> = attrs
            .iter()
            .filter_map(|attr| {
                let new_start = line_map.get(&attr.start_line).copied()?;
                let new_end = line_map.get(&attr.end_line).copied()?;
                Some(LineAttribution::new(
                    new_start,
                    new_end,
                    attr.author_id.clone(),
                    attr.overrode.clone(),
                ))
            })
            .collect();

        if !shifted.is_empty() {
            files.insert(file_path.clone(), shifted);
            file_contents.insert(file_path.clone(), current_content);
        }
    }

    if files.is_empty() {
        return Ok(());
    }

    let working_log = repo.storage.working_log_for_base_commit(current_head)?;
    remove_checkpoint_entries_for_files(&working_log, files.keys().cloned())?;
    merge_initial_replacing_paths_with_contents(
        &working_log,
        files,
        initial.prompts,
        initial.humans,
        file_contents,
        initial.sessions,
        None,
    )?;

    Ok(())
}

fn copy_initial_blobs(
    src_log: &PersistedWorkingLog,
    dst_log: &PersistedWorkingLog,
    initial: &InitialAttributions,
) -> Result<(), GitAiError> {
    if initial.file_blobs.is_empty() {
        return Ok(());
    }

    let dst_blobs = dst_log.dir.join("blobs");
    fs::create_dir_all(&dst_blobs)?;
    for blob_sha in initial.file_blobs.values() {
        let src = src_log.dir.join("blobs").join(blob_sha);
        let dst = dst_blobs.join(blob_sha);
        if src.exists() && !dst.exists() {
            fs::copy(src, dst)?;
        }
    }
    Ok(())
}

fn remove_checkpoint_entries_for_files<I>(
    working_log: &PersistedWorkingLog,
    files: I,
) -> Result<(), GitAiError>
where
    I: IntoIterator<Item = String>,
{
    let files: HashSet<String> = files.into_iter().collect();
    if files.is_empty() {
        return Ok(());
    }

    let checkpoints = working_log.read_all_checkpoints()?;
    if checkpoints.is_empty() {
        return Ok(());
    }

    let filtered = checkpoints
        .into_iter()
        .map(|mut checkpoint| {
            checkpoint
                .entries
                .retain(|entry| !files.contains(&entry.file));
            checkpoint
        })
        .filter(|checkpoint| !checkpoint.entries.is_empty())
        .collect::<Vec<_>>();
    working_log.write_all_checkpoints(&filtered)?;
    Ok(())
}

fn merge_initial_replacing_paths(
    working_log: &PersistedWorkingLog,
    mut source: InitialAttributions,
    replaced_paths: Option<&HashSet<String>>,
) -> Result<(), GitAiError> {
    if source.files.is_empty() && replaced_paths.is_none_or(HashSet::is_empty) {
        return Ok(());
    }

    let mut restored_paths: HashSet<String> = source.files.keys().cloned().collect();
    if let Some(replaced_paths) = replaced_paths {
        restored_paths.extend(replaced_paths.iter().cloned());
    }
    let mut target = working_log.read_initial_attributions();
    for path in &restored_paths {
        target.files.remove(path);
        target.file_blobs.remove(path);
    }

    target.files.extend(source.files.drain());
    target.file_blobs.extend(source.file_blobs.drain());
    target.prompts.extend(source.prompts.drain());
    target.humans.extend(source.humans);
    target.sessions.extend(source.sessions);
    trim_initial_metadata_to_referenced_authors(&mut target);
    working_log.write_initial(target)?;
    Ok(())
}

fn merge_initial_replacing_paths_with_contents(
    working_log: &PersistedWorkingLog,
    files: HashMap<String, Vec<LineAttribution>>,
    prompts: HashMap<String, PromptRecord>,
    humans: BTreeMap<String, HumanRecord>,
    file_contents: HashMap<String, String>,
    sessions: BTreeMap<String, SessionRecord>,
    replaced_paths: Option<&HashSet<String>>,
) -> Result<(), GitAiError> {
    let files: HashMap<String, Vec<LineAttribution>> = files
        .into_iter()
        .filter(|(_, attrs)| !attrs.is_empty())
        .collect();
    if files.is_empty() && replaced_paths.is_none_or(HashSet::is_empty) {
        return Ok(());
    }

    let mut file_blobs = HashMap::new();
    for file_path in files.keys() {
        let content = file_contents
            .get(file_path)
            .ok_or_else(|| PersistenceError::Io {
                operation: "Generic error",
                path: String::new(),
                kind: std::io::ErrorKind::NotFound,
                message: format!("stash restore missing file content snapshot for {file_path}"),
            })?;
        let blob_sha = working_log.persist_file_version(content)?;
        file_blobs.insert(file_path.clone(), blob_sha);
    }

    merge_initial_replacing_paths(
        working_log,
        InitialAttributions {
            files,
            prompts,
            file_blobs,
            humans,
            sessions,
        },
        replaced_paths,
    )
}

#[cfg(test)]
mod tests;
