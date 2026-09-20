use super::transport::exec_notes_transport;
use crate::config::{Config, NotesBackendKind};
use crate::error::GitAiError;
use crate::operations::git::find_repository_in_path;
use crate::operations::git::refs::AI_AUTHORSHIP_PUSH_REFSPEC;

pub fn send_authorship_notes(worktree: &str, destination: &str) -> Result<(), GitAiError> {
    let config = Config::fresh();
    if !config.feature_flags().send_pack_notes_sync
        || config.notes_backend_kind() != NotesBackendKind::GitNotes
    {
        return Ok(());
    }
    let repository = find_repository_in_path(worktree)?;
    if !repository.is_collection_allowed(&config) {
        return Ok(());
    }
    // Native send-pack uses the literal endpoint and has no pre-push hook.
    // Using push instead could apply URL rewrites or named-remote configuration.
    // Do not force or merge: a divergent notes ref must remain untouched.
    let mut args = repository.global_args_for_exec();
    args.extend([
        "send-pack".to_string(),
        "--quiet".to_string(),
        destination.to_string(),
        AI_AUTHORSHIP_PUSH_REFSPEC.to_string(),
    ]);
    exec_notes_transport(&args)?;
    Ok(())
}
