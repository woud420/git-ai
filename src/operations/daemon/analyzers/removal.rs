use crate::model::domain::{NormalizedCommand, SemanticEvent, WorktreeState};
use crate::operations::daemon::side_effect_helpers::parsed_invocation_for_normalized_command;
use crate::operations::git::oid::is_non_zero_oid;

pub(super) fn analyze_removal(
    cmd: &NormalizedCommand,
    worktree: Option<&WorktreeState>,
) -> Option<SemanticEvent> {
    if cmd.exit_code != 0 || !cmd.trace_derived {
        return None;
    }
    let worktree = worktree?;
    if worktree.last_updated_ns > cmd.started_at_ns {
        return None;
    }
    let head = worktree
        .head
        .as_ref()
        .filter(|head| is_non_zero_oid(head))?;
    let parsed = parsed_invocation_for_normalized_command(cmd);
    if parsed.command.as_deref() != Some("rm") {
        return None;
    }
    let mut globals = parsed.global_args.iter();
    while let Some(arg) = globals.next() {
        match arg.as_str() {
            "-C" if globals.next().is_some() => {}
            value if value.starts_with("-C") && value.len() > 2 => {}
            _ => return None,
        }
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
        head: head.clone(),
        files,
    })
}

#[cfg(test)]
mod tests;
