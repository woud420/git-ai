use crate::error::GitAiError;
use crate::model::domain::SemanticEvent;
use crate::operations::git::find_repository_in_path;

pub(super) fn apply(worktree: &str, event: &SemanticEvent) -> Result<bool, GitAiError> {
    match event {
        SemanticEvent::WorkingTreeFilesRemoved { head, files } => {
            let repo = find_repository_in_path(worktree)?;
            super::working_log_discard::remove_working_log_attributions_for_files(
                &repo, head, files,
            )?;
        }
        SemanticEvent::OrphanBranchCreated {
            old_head,
            discard_tracked,
            ..
        } => {
            super::orphan_branch::migrate_working_log(worktree, old_head, *discard_tracked)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}
