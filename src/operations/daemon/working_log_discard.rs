use super::side_effect_helpers::matches_any_pathspec;
use crate::clients::git_cli::exec_git_stdin;
use crate::error::GitAiError;
use crate::model::domain::IndexWriteEvidence;
use crate::model::working_log::InitialAttributions;
use crate::operations::git::find_repository_in_path;
use crate::operations::git::refs::parse_batch_check_blob_oid;
use crate::operations::git::repository::Repository;

pub(super) fn apply_path_discard(
    worktree: &str,
    head: &str,
    path: &str,
    index_write: &IndexWriteEvidence,
) -> Result<(), GitAiError> {
    let repo = find_repository_in_path(worktree)?;
    if !repo.is_collection_allowed(&crate::config::Config::fresh()) {
        return Ok(());
    }
    // An explicit path discard can still write an alternate index. Only
    // Git's recorded write to this worktree's index proves default staged
    // evidence was discarded too; inspecting the later index cannot prove it.
    if !matches!(index_write, IndexWriteEvidence::Exact(path) if path == &repo.path().join("index.lock"))
    {
        return Ok(());
    }
    if !repo.storage.has_working_log(head) {
        return Ok(());
    }
    // A directory operation can skip descendants. Require an exact immutable blob
    // before treating the successful command as evidence that this path changed.
    let mut args = repo.global_args_for_exec();
    args.extend([
        "--no-replace-objects".to_string(),
        "cat-file".to_string(),
        "--batch-check=%(objectname) %(objecttype)".to_string(),
    ]);
    let output = exec_git_stdin(&args, format!("{head}:{path}\n").as_bytes())?;
    let stdout = String::from_utf8(output.stdout)?;
    let mut records = stdout.lines();
    if records
        .next()
        .and_then(parse_batch_check_blob_oid)
        .is_some()
        && records.next().is_none()
    {
        // Match only this file: descendant evidence can belong to a different
        // path shape, including one exposed through replacement objects.
        remove_matching_attributions(&repo, head, |file| file == path)?;
    }
    Ok(())
}

pub fn remove_working_log_attributions_for_pathspecs(
    repository: &Repository,
    head: &str,
    pathspecs: &[String],
) -> Result<(), GitAiError> {
    remove_matching_attributions(repository, head, |file| {
        matches_any_pathspec(file, pathspecs)
    })
}

pub(super) fn remove_working_log_attributions_for_files(
    repository: &Repository,
    head: &str,
    files: &[String],
) -> Result<(), GitAiError> {
    if !repository.storage.has_working_log(head) {
        return Ok(());
    }
    let files = files
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    remove_matching_attributions(repository, head, |file| files.contains(file))
}

pub(super) fn remove_matching_attributions(
    repository: &Repository,
    head: &str,
    matches: impl Fn(&str) -> bool,
) -> Result<(), GitAiError> {
    let working_log = repository.storage.working_log_for_base_commit(head)?;

    let initial = working_log.read_initial_attributions();
    if !initial.files.is_empty() {
        let filtered_files = initial
            .files
            .into_iter()
            .filter(|(file, _)| !matches(file))
            .collect();
        let mut filtered_blobs = initial.file_blobs;
        filtered_blobs.retain(|file, _| !matches(file));
        working_log.write_initial(InitialAttributions {
            files: filtered_files,
            prompts: initial.prompts,
            file_blobs: filtered_blobs,
            humans: initial.humans,
            sessions: initial.sessions,
        })?;
    }

    let checkpoints = working_log.read_all_checkpoints()?;
    let filtered: Vec<_> = checkpoints
        .into_iter()
        .map(|mut checkpoint| {
            checkpoint.entries.retain(|entry| !matches(&entry.file));
            checkpoint
        })
        .filter(|checkpoint| !checkpoint.entries.is_empty())
        .collect();
    working_log.write_all_checkpoints(&filtered)?;
    Ok(())
}
