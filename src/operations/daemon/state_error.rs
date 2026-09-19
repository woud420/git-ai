use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;

pub(super) fn state_error(kind: std::io::ErrorKind, message: String) -> GitAiError {
    // Daemon status and replay records persist the historical display prefix.
    PersistenceError::Io {
        operation: "Generic error",
        path: String::new(),
        kind,
        message,
    }
    .into()
}

/// `Some(error)` when a `commit` invocation exited 0 but ref-cursor
/// enrichment (`RefCursor::enrich_commit`, including its existing
/// cold-seed-clamp and ingress-hint mitigations) still found no HEAD
/// transition, and this isn't one of the known legitimate no-op shapes.
/// Reuses `commit_skip_reason`'s exact classification of "opaque outcome for
/// a commit" so the diagnostic-only skip reason recorded in the completion
/// log and this fail-loud gate can never drift apart.
///
/// It deliberately does not rescan the reflog: message/timestamp matching is
/// explicitly rejected by the ingestion spec as unsound. This only makes the
/// existing fail-closed outcome observable; it does not change enrichment.
///
/// Known legitimate exceptions (must NOT error):
/// - `--dry-run`: never touches the reflog, by design.
/// - non-zero exit: excluded by the explicit `exit_code == 0` check (a
///   rejected/empty/conflicted commit attempt routinely produces no HEAD
///   move and is not a bug).
///
/// Not handled here (documented limitations, not attempted): a repo with
/// `core.logAllRefUpdates=false` has no reflog for ref-cursor to read at all,
/// so every commit there would fail loud; and a `commit --amend` that
/// reproduces a byte-identical commit object (requires a fixed
/// `GIT_COMMITTER_DATE`) moves no ref and writes no reflog entry. Both are
/// pre-existing constraints of the reflog-cursor attribution model itself,
/// not introduced by this check.
pub(crate) fn commit_enrichment_unrecoverable_error(
    cmd: &crate::model::domain::NormalizedCommand,
    events: &[crate::model::domain::SemanticEvent],
) -> Option<GitAiError> {
    if cmd.exit_code != 0 {
        return None;
    }
    crate::operations::daemon::actor_coordinator_seq::commit_skip_reason(
        cmd.primary_command.as_deref(),
        events,
    )?;
    let command_args = crate::operations::daemon::analyzers::command_args(cmd);
    if crate::operations::git::cli_parser::is_dry_run(&command_args)
        || command_args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--short" | "--porcelain" | "--long"))
    {
        return None;
    }
    // Collection is opt-in per repository: a note was never going to be
    // written for a non-allowed repo, so a lost enrichment there is not an
    // attribution loss and must not raise error telemetry. This resolve+check
    // runs only on the already-rare failure path, never per healthy commit.
    if let Some(worktree) = cmd.worktree.as_deref()
        && let Ok(repo) =
            crate::operations::git::find_repository_in_path(&worktree.to_string_lossy())
        && !repo.is_collection_allowed(&crate::config::Config::fresh())
    {
        return None;
    }
    Some(state_error(
        std::io::ErrorKind::InvalidData,
        format!(
            "commit sid={} exited 0 but ref-cursor enrichment found no HEAD transition; no AI \
         attribution note was written for this commit (see \
         docs/architecture/daemon-trace2-ingestion-spec.md)",
            cmd.root_sid
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_state_errors_preserve_existing_display_and_retryability() {
        for message in [
            "family not found",
            "trace payload missing event",
            "global actor apply send failed",
        ] {
            let error = state_error(std::io::ErrorKind::InvalidData, message.into());
            let GitAiError::Persistence(inner) = &error else {
                panic!("state errors must expose structured persistence data");
            };
            assert_eq!(
                error.to_string(),
                GitAiError::Generic(message.into()).to_string()
            );
            assert!(matches!(
                inner.retryability(),
                crate::error::Retryability::Terminal
            ));
        }
    }
}
