use crate::model::domain::{NormalizedCommand, WorktreeState};
use crate::model::git_oid::is_non_zero_oid;
use crate::operations::git::cli_parser::ParsedGitInvocation;

pub(super) fn ordered_head<'a>(
    cmd: &NormalizedCommand,
    worktree: Option<&'a WorktreeState>,
) -> Option<&'a str> {
    if cmd.exit_code != 0 || !cmd.trace_derived {
        return None;
    }
    let worktree = worktree?;
    if worktree.last_updated_ns > cmd.started_at_ns {
        return None;
    }
    worktree
        .head
        .as_deref()
        .filter(|head| is_non_zero_oid(head))
}

pub(super) fn has_only_worktree_globals(parsed: &ParsedGitInvocation) -> bool {
    let mut globals = parsed.global_args.iter();
    while let Some(arg) = globals.next() {
        match arg.as_str() {
            "-C" if globals.next().is_some() => {}
            value if value.starts_with("-C") && value.len() > 2 => {}
            _ => return false,
        }
    }
    true
}
