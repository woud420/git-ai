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
    if parsed.command.as_deref() != Some("restore") {
        return None;
    }
    let (source, path) = match parsed.command_args.as_slice() {
        // A worktree-only restore can leave the same edit staged. Discarding
        // its evidence would misattribute a later commit of that index entry.
        [
            source_flag,
            source,
            staged_flag,
            worktree_flag,
            separator,
            path,
        ] if source_flag == "--source"
            && staged_flag == "--staged"
            && worktree_flag == "--worktree"
            && separator == "--" =>
        {
            (source.as_str(), path.as_str())
        }
        [source_flag, staged_flag, worktree_flag, separator, path]
            if staged_flag == "--staged" && worktree_flag == "--worktree" && separator == "--" =>
        {
            (source_flag.strip_prefix("--source=")?, path.as_str())
        }
        _ => return None,
    };
    if source != head {
        return None;
    }

    let path = proven_literal_worktree_path(&parsed.global_args, worktree, path)?;
    Some(SemanticEvent::WorkingLogPathDiscarded {
        base_commit: head.clone(),
        path,
    })
}
