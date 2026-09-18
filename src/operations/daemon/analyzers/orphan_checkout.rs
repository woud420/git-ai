use super::workspace_evidence::{has_only_worktree_globals, ordered_head};
use crate::model::domain::{NormalizedCommand, SemanticEvent, WorktreeState};
use crate::operations::daemon::side_effect_helpers::parsed_invocation_for_normalized_command;

pub(super) fn analyze_orphan_checkout(
    cmd: &NormalizedCommand,
    worktree: Option<&WorktreeState>,
) -> Option<SemanticEvent> {
    let old_head = ordered_head(cmd, worktree)?;
    if !cmd.ref_changes.is_empty() || !cmd.observed_child_commands.is_empty() {
        return None;
    }
    let parsed = parsed_invocation_for_normalized_command(cmd);
    if !matches!(parsed.command.as_deref(), Some("checkout" | "switch"))
        || !has_only_worktree_globals(&parsed)
    {
        return None;
    }
    let [flag, branch] = parsed.command_args.as_slice() else {
        return None;
    };
    if flag != "--orphan" || branch.is_empty() || branch.starts_with('-') {
        return None;
    }
    // These exact forms preserve pending edits; switch removes old HEAD paths.
    // A different start-point or --force can replace checkpointed content.
    Some(SemanticEvent::OrphanBranchCreated {
        old_head: old_head.to_string(),
        branch: format!("refs/heads/{branch}"),
        discard_tracked: parsed.command.as_deref() == Some("switch"),
    })
}

#[cfg(test)]
mod tests;
