use super::workspace_evidence::{has_only_worktree_globals, ordered_head};
use crate::model::domain::{NormalizedCommand, SemanticEvent, WorktreeState};
use crate::operations::daemon::side_effect_helpers::parsed_invocation_for_normalized_command;

pub(super) fn analyze_removal(
    cmd: &NormalizedCommand,
    worktree: Option<&WorktreeState>,
) -> Option<SemanticEvent> {
    let head = ordered_head(cmd, worktree)?;
    let parsed = parsed_invocation_for_normalized_command(cmd);
    if parsed.command.as_deref() != Some("rm") || !has_only_worktree_globals(&parsed) {
        return None;
    }
    let separator = parsed.command_args.iter().position(|arg| arg == "--")?;
    if parsed.command_args[..separator]
        .iter()
        .any(|arg| !matches!(arg.as_str(), "-f" | "--force" | "-q" | "--quiet"))
    {
        return None;
    }
    // Trace2 does not preserve the invocation subdirectory or the old index.
    // Root-literal, non-recursive removals prove exactly which files vanished;
    // directory, cached, dry-run and ignore-unmatch forms do not.
    let files = parsed.command_args[separator + 1..]
        .iter()
        .map(|arg| {
            let file = arg
                .strip_prefix(":(top,literal)")
                .or_else(|| arg.strip_prefix(":(literal,top)"))?;
            if file.contains(['\\', '\0'])
                || file.split('/').any(|part| matches!(part, "" | "." | ".."))
            {
                return None;
            }
            Some(file.to_string())
        })
        .collect::<Option<Vec<_>>>()?;
    if files.is_empty() {
        return None;
    }
    Some(SemanticEvent::WorkingTreeFilesRemoved {
        head: head.to_string(),
        files,
    })
}

#[cfg(test)]
mod tests;
