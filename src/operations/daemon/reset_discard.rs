use super::git_op_side_effects::remove_working_log_attributions_for_pathspecs;
use super::working_log_discard::tracked_recorded_paths;
use crate::error::GitAiError;
use crate::model::domain::{IndexWriteEvidence, NormalizedCommand};
use crate::operations::git::repository::Repository;

pub(super) fn discard_same_head_tracked_paths(
    repo: &Repository,
    head: &str,
    cmd: &NormalizedCommand,
) -> Result<(), GitAiError> {
    if !repo.is_collection_allowed(&crate::config::Config::fresh()) {
        return Ok(());
    }
    // Replacement trees can leave canonical tracked paths untracked. The
    // command must prove its object view; later replacement refs cannot.
    let parsed = super::side_effect_helpers::parsed_invocation_for_normalized_command(cmd);
    if !parsed
        .global_args
        .iter()
        .any(|arg| arg == "--no-replace-objects")
    {
        return Ok(());
    }
    // Version 2 cannot encode skip-worktree entries. Later index reads cannot
    // establish whether the reset actually discarded a sparse working edit.
    if !cmd.index_v2
        || !matches!(&cmd.index_write, IndexWriteEvidence::Exact(path) if path == &repo.path().join("index.lock"))
        || !repo.storage.has_working_log(head)
    {
        return Ok(());
    }
    let tracked: Vec<_> = tracked_recorded_paths(repo, head, "same-HEAD reset")?
        .into_iter()
        .collect();
    if !tracked.is_empty() {
        // The family sequencer applies this before later checkpoint writes;
        // retaining other paths preserves untracked files that Git left intact.
        remove_working_log_attributions_for_pathspecs(repo, head, &tracked)?;
    }
    Ok(())
}
