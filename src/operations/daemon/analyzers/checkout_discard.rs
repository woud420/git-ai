use super::literal_path::proven_literal_worktree_path;
use crate::model::domain::{IndexWriteEvidence, NormalizedCommand, SemanticEvent};
use crate::model::git_oid::is_non_zero_oid;
use crate::operations::git::cli_parser::parse_git_cli_args;
use std::collections::HashMap;

pub(crate) fn event(
    cmd: &NormalizedCommand,
    refs: &HashMap<String, String>,
) -> Option<SemanticEvent> {
    if cmd.exit_code != 0
        || cmd.raw_argv.is_empty()
        || !matches!(cmd.index_write, IndexWriteEvidence::Exact(_))
    {
        return None;
    }
    let worktree = cmd.worktree.as_deref()?;
    let head = refs.get("HEAD").filter(|head| is_non_zero_oid(head))?;
    let parsed = parse_git_cli_args(&super::normalized_args(&cmd.raw_argv));
    if parsed.command.as_deref() != Some("checkout") {
        return None;
    }
    let [source, separator, path] = parsed.command_args.as_slice() else {
        return None;
    };
    if separator != "--" {
        return None;
    }
    if source != head {
        return None;
    }

    let path = proven_literal_worktree_path(&parsed.global_args, worktree, path)?;
    Some(SemanticEvent::WorkingLogPathDiscarded {
        base_commit: head.clone(),
        path,
    })
}
