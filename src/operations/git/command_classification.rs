use super::command_policy;

/// Returns true if the given git subcommand is guaranteed to never mutate
/// repository state (refs, objects, config, worktree). Used to skip expensive
/// trace2 ingestion work and suppress trace2 emission for read-only commands.
pub fn is_definitely_read_only_command(command: &str) -> bool {
    command_policy::is_definitely_read_only_command(command)
}

/// Returns true if the full Git invocation is guaranteed to never mutate
/// repository state. This is intentionally conservative: unknown flags on
/// mixed read/write commands are treated as potentially mutating.
///
/// Extends `is_definitely_read_only_command` to handle commands like `stash`,
/// `worktree`, and `notes` whose read-only status depends on the subcommand:
/// - `git stash list` / `git stash show` are read-only; `pop`/`apply` are not
/// - `git worktree list` is read-only; `add`/`remove` are not
/// - `git notes show` / `git notes list` / `git notes get-ref` are read-only;
///   `add`/`append`/`remove` are not
///
/// IDEs like Zed issue thousands of `stash list` and `worktree list` calls
/// per minute for their git panel UI. These must be identified as read-only
/// so the trace2 pipeline can drop them without processing.
pub fn is_definitely_read_only_git_invocation(command: &str, command_args: &[String]) -> bool {
    if is_definitely_read_only_command(command) {
        return true;
    }

    match command {
        "branch" => branch_invocation_is_read_only(command_args),
        "notes" => matches!(
            command_args.first().map(String::as_str),
            Some("show" | "list" | "get-ref")
        ),
        "remote" => remote_invocation_is_read_only(command_args),
        "stash" => stash_invocation_is_read_only(command_args),
        "tag" => tag_invocation_is_read_only(command_args),
        "worktree" => worktree_invocation_is_read_only(command_args),
        _ => false,
    }
}

/// Returns true when a Git command may mutate repository state and therefore
/// must be treated as an ordered trace2 root.
pub fn may_mutate_repo_state_command(command: &str) -> bool {
    command_policy::may_mutate_repo_state_command(command)
}

/// Returns true when a full Git invocation may mutate repository state and
/// therefore must be treated as an ordered trace2 root.
pub fn git_invocation_may_mutate_repo_state(command: &str, command_args: &[String]) -> bool {
    may_mutate_repo_state_command(command)
        && !is_definitely_read_only_git_invocation(command, command_args)
}

/// Returns true when a Git command must be ordered inside an existing repo
/// family. Commands like clone/init may mutate state, but they establish or
/// target a different repository context and must not be sequenced under the
/// launching repository family.
pub fn participates_in_family_sequencer_command(command: &str) -> bool {
    command_policy::participates_in_family_sequencer_command(command)
}

/// Returns true when a full Git invocation must be ordered inside an existing
/// repo family.
pub fn git_invocation_participates_in_family_sequencer(
    command: &str,
    command_args: &[String],
) -> bool {
    participates_in_family_sequencer_command(command)
        && git_invocation_may_mutate_repo_state(command, command_args)
}

fn branch_invocation_is_read_only(args: &[String]) -> bool {
    if args.is_empty() {
        return true;
    }

    let mut idx = 0usize;
    let mut saw_list_mode = false;
    while idx < args.len() {
        let arg = args[idx].as_str();
        if arg == "--" {
            return false;
        }
        if branch_arg_is_mutating(arg) {
            return false;
        }
        if arg == "--list" || arg == "-l" {
            saw_list_mode = true;
            idx += 1;
            continue;
        }
        if branch_arg_takes_optional_query_value(arg) {
            idx += 1;
            if args
                .get(idx)
                .is_some_and(|next| !next.starts_with('-') && next != "--")
            {
                idx += 1;
            }
            continue;
        }
        if branch_arg_takes_required_query_value(arg) {
            idx += 1;
            if args
                .get(idx)
                .is_some_and(|next| !next.starts_with('-') && next != "--")
            {
                idx += 1;
                continue;
            }
            return false;
        }
        if branch_arg_is_read_only_flag(arg) || branch_arg_is_inline_query_option(arg) {
            idx += 1;
            continue;
        }
        if !arg.starts_with('-') && saw_list_mode {
            idx += 1;
            continue;
        }
        return false;
    }
    true
}

fn branch_arg_is_mutating(arg: &str) -> bool {
    matches!(
        arg,
        "-d" | "-D"
            | "--delete"
            | "-m"
            | "-M"
            | "--move"
            | "-c"
            | "-C"
            | "--copy"
            | "--set-upstream-to"
            | "--unset-upstream"
            | "--edit-description"
            | "--create-reflog"
    ) || arg.starts_with("--set-upstream-to=")
}

fn branch_arg_takes_optional_query_value(arg: &str) -> bool {
    matches!(
        arg,
        "--contains" | "--no-contains" | "--merged" | "--no-merged"
    )
}

fn branch_arg_takes_required_query_value(arg: &str) -> bool {
    matches!(arg, "--points-at" | "--sort" | "--format" | "--abbrev")
}

fn branch_arg_is_inline_query_option(arg: &str) -> bool {
    arg.starts_with("--points-at=")
        || arg.starts_with("--sort=")
        || arg.starts_with("--format=")
        || arg.starts_with("--color=")
        || arg.starts_with("--column=")
        || arg.starts_with("--abbrev=")
}

fn branch_arg_is_read_only_flag(arg: &str) -> bool {
    matches!(
        arg,
        "-a" | "--all"
            | "-r"
            | "--remotes"
            | "-v"
            | "-vv"
            | "--verbose"
            | "-q"
            | "--quiet"
            | "--show-current"
            | "--color"
            | "--no-color"
            | "--column"
            | "--ignore-case"
            | "--no-column"
            | "--no-abbrev"
            | "--omit-empty"
    )
}

fn remote_invocation_is_read_only(args: &[String]) -> bool {
    if args.is_empty() {
        return true;
    }
    let mut idx = 0usize;
    while args
        .get(idx)
        .is_some_and(|arg| matches!(arg.as_str(), "-v" | "--verbose"))
    {
        idx += 1;
    }
    matches!(
        args.get(idx).map(String::as_str),
        None | Some("show" | "get-url")
    )
}

fn stash_invocation_is_read_only(args: &[String]) -> bool {
    let subcommand = args.iter().find(|arg| !arg.starts_with('-'));
    matches!(subcommand.map(String::as_str), Some("list" | "show"))
}

fn tag_invocation_is_read_only(args: &[String]) -> bool {
    if args.is_empty() {
        return true;
    }

    let mut idx = 0usize;
    let mut query_mode = false;
    while idx < args.len() {
        let arg = args[idx].as_str();
        if arg == "--" {
            return false;
        }
        if tag_arg_is_mutating(arg) {
            return false;
        }
        if arg == "-l" || arg == "--list" || arg == "-v" || arg == "--verify" {
            query_mode = true;
            idx += 1;
            continue;
        }
        if tag_arg_takes_optional_query_value(arg) {
            query_mode = true;
            idx += 1;
            if args
                .get(idx)
                .is_some_and(|next| !next.starts_with('-') && next != "--")
            {
                idx += 1;
            }
            continue;
        }
        if tag_arg_takes_required_query_value(arg) {
            query_mode = true;
            idx += 1;
            if args
                .get(idx)
                .is_some_and(|next| !next.starts_with('-') && next != "--")
            {
                idx += 1;
                continue;
            }
            return false;
        }
        if tag_arg_is_inline_query_option(arg) || tag_arg_is_read_only_flag(arg) {
            query_mode = true;
            idx += 1;
            continue;
        }
        if !arg.starts_with('-') && query_mode {
            idx += 1;
            continue;
        }
        return false;
    }
    true
}

fn tag_arg_is_mutating(arg: &str) -> bool {
    matches!(
        arg,
        "-a" | "--annotate"
            | "-s"
            | "--sign"
            | "-u"
            | "--local-user"
            | "-m"
            | "--message"
            | "-F"
            | "--file"
            | "-d"
            | "--delete"
            | "-f"
            | "--force"
    ) || arg.starts_with("--local-user=")
        || arg.starts_with("--message=")
        || arg.starts_with("--file=")
}

fn tag_arg_takes_optional_query_value(arg: &str) -> bool {
    matches!(
        arg,
        "--contains" | "--no-contains" | "--merged" | "--no-merged"
    )
}

fn tag_arg_takes_required_query_value(arg: &str) -> bool {
    matches!(arg, "--points-at" | "--sort" | "--format")
}

fn tag_arg_is_inline_query_option(arg: &str) -> bool {
    arg.starts_with("--points-at=")
        || arg.starts_with("--sort=")
        || arg.starts_with("--format=")
        || arg.starts_with("--color=")
        || arg.starts_with("--column=")
}

fn tag_arg_is_read_only_flag(arg: &str) -> bool {
    matches!(
        arg,
        "-n" | "--color" | "--column" | "--ignore-case" | "--no-column" | "--no-color"
    ) || arg.starts_with("-n")
}

fn worktree_invocation_is_read_only(args: &[String]) -> bool {
    let subcommand = args.iter().find(|arg| !arg.starts_with('-'));
    matches!(subcommand.map(String::as_str), Some("list"))
}

#[path = "command_classification_tests.rs"]
#[cfg(test)]
mod tests;
