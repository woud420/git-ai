use super::side_effect_helpers::matches_any_pathspec;
use crate::clients::git_cli::exec_git_stdin;
use crate::error::GitAiError;
use crate::model::domain::IndexWriteEvidence;
use crate::model::working_log::InitialAttributions;
use crate::operations::git::find_repository_in_path;
use crate::operations::git::refs::parse_batch_check_blob_oid;
use crate::operations::git::repository::Repository;
use std::collections::BTreeSet;

const MAX_PATHS: usize = 4096;
const MAX_QUERY_BYTES: usize = 1024 * 1024;

/// Runs one `cat-file --batch-check` in the canonical object view (replacement
/// refs ignored): an `<oid> <type>` record per queried object, contents unread.
pub(super) fn batch_check_objects(repo: &Repository, queries: &str) -> Result<String, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.extend([
        "--no-replace-objects".to_string(),
        "cat-file".to_string(),
        "--batch-check=%(objectname) %(objecttype)".to_string(),
    ]);
    let output = exec_git_stdin(&args, queries.as_bytes())?;
    Ok(String::from_utf8(output.stdout)?)
}

/// Every path the working log for `head` records, in INITIAL or a checkpoint.
pub(super) fn recorded_paths(
    repo: &Repository,
    head: &str,
) -> Result<BTreeSet<String>, GitAiError> {
    let working_log = repo.storage.working_log_for_base_commit(head)?;
    let initial = working_log.read_initial_attributions();
    let checkpoints = working_log.read_all_checkpoints()?;
    Ok(initial
        .files
        .into_keys()
        .chain(
            checkpoints
                .into_iter()
                .flat_map(|checkpoint| checkpoint.entries.into_iter().map(|entry| entry.file)),
        )
        .collect())
}

/// One `{head}:{path}` batch-check query per path, appended to `prefix`.
/// `None` when a path cannot be newline-framed or the batch is over budget.
pub(super) fn path_queries(prefix: String, head: &str, paths: &BTreeSet<String>) -> Option<String> {
    let query_bytes = prefix.len()
        + paths
            .iter()
            .map(|path| head.len() + path.len() + 2)
            .sum::<usize>();
    if paths.len() > MAX_PATHS
        || paths.iter().any(|path| path.contains(['\n', '\r', '\0']))
        || query_bytes > MAX_QUERY_BYTES
    {
        return None;
    }
    let mut queries = prefix;
    queries.extend(paths.iter().map(|path| format!("{head}:{path}\n")));
    Some(queries)
}

/// Recorded paths that are blobs in `head`'s tree. Empty, with a warning
/// naming `operation`, when that cannot be established for every path.
pub(super) fn tracked_recorded_paths(
    repo: &Repository,
    head: &str,
    operation: &str,
) -> Result<BTreeSet<String>, GitAiError> {
    let paths = recorded_paths(repo, head)?;
    if paths.is_empty() {
        return Ok(paths);
    }
    let Some(queries) = path_queries(String::new(), head, &paths) else {
        tracing::warn!(
            head,
            "{operation} attribution skipped: path query budget or encoding"
        );
        return Ok(BTreeSet::new());
    };

    // The index/worktree may already contain later edits. Resolve only recorded
    // paths in the command's immutable tree, without reading blob contents.
    let stdout = batch_check_objects(repo, &queries)?;
    let records: Vec<_> = stdout.lines().collect();
    if records.len() != paths.len() {
        tracing::warn!(
            head,
            "{operation} attribution skipped: incomplete path evidence"
        );
        return Ok(BTreeSet::new());
    }
    Ok(paths
        .into_iter()
        .zip(records)
        .filter(|(_, record)| parse_batch_check_blob_oid(record).is_some())
        .map(|(path, _)| path)
        .collect())
}

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
    let stdout = batch_check_objects(&repo, &format!("{head}:{path}\n"))?;
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
