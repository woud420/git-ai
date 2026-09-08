use super::*;
use crate::model::working_log::WorkingLogEntry;
use crate::operations::authorship::checkpoint_history::{
    historical_attributions, restore_matching_lines,
};
use crate::operations::authorship::virtual_attribution::{
    VirtualAttributions, checkout_merge_rebased_content,
};
use crate::operations::git::repository::batch_read_paths_at_treeishes;
use std::collections::BTreeSet;

pub(super) fn partition_stash_attributions(
    repo: &Repository,
    stash_sha: &str,
    head_sha: &str,
    pathspecs: &[String],
    keep_index: bool,
) -> Result<(), GitAiError> {
    if !repo.storage.has_working_log(head_sha) {
        return Ok(());
    }
    let va =
        VirtualAttributions::from_persisted_working_log(repo.clone(), head_sha.to_owned(), None)?;
    let live_log = repo.storage.working_log_for_base_commit(head_sha)?;
    let initial = live_log.read_initial_attributions();
    let checkpoints = live_log.read_all_checkpoints()?;
    let mut history: HashMap<String, Vec<&WorkingLogEntry>> = HashMap::new();
    for checkpoint in &checkpoints {
        for entry in &checkpoint.entries {
            history.entry(entry.file.clone()).or_default().push(entry);
        }
    }
    let candidates: HashSet<_> = history.keys().chain(initial.files.keys()).collect();
    let paths: Vec<_> = candidates
        .into_iter()
        .filter(|path| pathspecs.is_empty() || path_matches_any(path, pathspecs))
        .cloned()
        .collect();
    if paths.is_empty() {
        return Ok(());
    }
    let untracked = format!("{stash_sha}^3");
    let baseline = if keep_index {
        format!("{stash_sha}^2")
    } else {
        head_sha.to_owned()
    };
    let requests: Vec<_> = paths
        .iter()
        .flat_map(|path| {
            [stash_sha, head_sha, &untracked, &baseline]
                .into_iter()
                .map(move |tree| (tree.to_owned(), path.clone()))
        })
        .collect();
    let snapshots = batch_read_paths_at_treeishes(repo, &requests)?;
    let mut stashed_contents = HashMap::new();
    let mut live_contents = HashMap::new();
    for path in paths {
        let Some(observed) = va.get_file_content(&path) else {
            continue;
        };
        let Some(stashed) = snapshots
            .get(&(stash_sha.to_owned(), path.clone()))
            .or_else(|| snapshots.get(&(untracked.clone(), path.clone())))
        else {
            continue;
        };
        if snapshots.get(&(head_sha.to_owned(), path.clone())) == Some(stashed) {
            continue;
        }
        let target = snapshots
            .get(&(baseline.clone(), path.clone()))
            .map(String::as_str)
            .unwrap_or("");
        let live = checkout_merge_rebased_content(stashed, target, observed);
        stashed_contents.insert(path.clone(), stashed.clone());
        live_contents.insert(path, live);
    }
    let affected: HashSet<_> = stashed_contents.keys().cloned().collect();
    if affected.is_empty() {
        return Ok(());
    }
    let stashed = project(&va, &live_log, &initial, &history, &stashed_contents, false)?;
    let stash_log = working_log_for_dir(repo, stash_entry_dir(repo, stash_sha), head_sha);
    stash_log.write_initial_attributions_with_contents(
        stashed.files,
        stashed.prompts,
        stashed.humans,
        stashed_contents,
        stashed.sessions,
    )?;
    let live = project(
        &va,
        &live_log,
        &initial,
        &history,
        &live_contents,
        keep_index,
    )?;
    merge_initial_replacing_paths_with_contents(
        &live_log,
        live.files,
        live.prompts,
        live.humans,
        live_contents,
        live.sessions,
        Some(&affected),
    )?;
    remove_checkpoint_entries_for_files(&live_log, affected)?;
    Ok(())
}

fn project(
    metadata: &VirtualAttributions,
    working_log: &PersistedWorkingLog,
    initial: &InitialAttributions,
    history: &HashMap<String, Vec<&WorkingLogEntry>>,
    target_contents: &HashMap<String, String>,
    recover_history: bool,
) -> Result<InitialAttributions, GitAiError> {
    let mut attributions = HashMap::new();
    for (path, target) in target_contents {
        let lines: Vec<_> = (1..=target.lines().count() as u32).collect();
        let attrs = if recover_history {
            historical_attributions(
                working_log,
                initial,
                path,
                target,
                &lines,
                history.get(path).map(Vec::as_slice).unwrap_or(&[]),
            )?
        } else {
            // The stash worktree can include edits never checkpointed. Only the
            // index partition may select an older captured version of the file.
            let mut matched = Vec::new();
            if let (Some(content), Some(attrs)) = (
                metadata.get_file_content(path),
                metadata.get_line_attributions(path),
            ) {
                restore_matching_lines(
                    content,
                    target,
                    attrs,
                    &mut BTreeSet::from_iter(lines),
                    &mut matched,
                );
            }
            matched
        };
        attributions.insert(path.clone(), (Vec::new(), attrs));
    }
    let mut projected = VirtualAttributions::new(
        metadata.repo().clone(),
        metadata.base_commit().to_owned(),
        attributions,
        HashMap::new(),
        0,
    );
    projected.prompts = metadata.prompts.clone();
    projected.humans = metadata.humans.clone();
    projected.sessions = metadata.sessions.clone();
    Ok(projected.to_initial_working_log_only())
}
