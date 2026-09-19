use super::literal_path::proven_literal_worktree_path;
use crate::model::domain::{IndexWriteEvidence, NormalizedCommand, SemanticEvent};
use crate::model::git_oid::is_non_zero_oid;
use crate::operations::git::cli_parser::parse_git_cli_args;
use std::collections::HashMap;
use std::path::Path;

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
    if parsed.command.as_deref() != Some("mv") {
        return None;
    }
    let [separator, source, destination] = parsed.command_args.as_slice() else {
        return None;
    };
    // Git treats "." as an existing directory. Other destinations depend on
    // operation-time filesystem shape, even when written with a trailing slash.
    // A relative source forces the shared helper to prove the root via -C.
    if separator != "--" || destination != "." || Path::new(source).is_absolute() {
        return None;
    }
    let source = proven_literal_worktree_path(&parsed.global_args, worktree, source)?;
    let (_, destination) = source.rsplit_once('/')?;
    Some(SemanticEvent::WorkingLogPathMoved {
        base_commit: head.clone(),
        destination: destination.to_string(),
        source,
    })
}
