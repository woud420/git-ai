use super::working_log_discard::{
    remove_matching_attributions as remove_working_log_attributions_matching,
    tracked_recorded_paths,
};
use crate::error::GitAiError;
use crate::model::domain::{IndexWriteEvidence, NormalizedCommand};
use crate::operations::git::repository::Repository;

pub(super) fn apply(
    repo: &Repository,
    head: &str,
    cmd: &NormalizedCommand,
) -> Result<(), GitAiError> {
    if !repo.is_collection_allowed(&crate::config::Config::fresh()) {
        return Ok(());
    }
    // Version 2 cannot encode skip-worktree entries. Later index reads cannot
    // establish whether the switch actually discarded a sparse working edit.
    if !cmd.index_v2
        || !matches!(&cmd.index_write, IndexWriteEvidence::Exact(path) if path == &repo.path().join("index.lock"))
        || !repo.storage.has_working_log(head)
    {
        return Ok(());
    }
    let tracked = tracked_recorded_paths(repo, head, "same-OID switch")?;
    if !tracked.is_empty() {
        // The family sequencer applies this before later checkpoint writes;
        // retaining other paths preserves untracked files that Git left intact.
        remove_working_log_attributions_matching(repo, head, |path| tracked.contains(path))?;
    }
    Ok(())
}
