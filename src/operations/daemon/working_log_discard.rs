use super::side_effect_helpers::matches_any_pathspec;
use crate::error::GitAiError;
use crate::model::working_log::InitialAttributions;
use crate::operations::git::repository::Repository;

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
