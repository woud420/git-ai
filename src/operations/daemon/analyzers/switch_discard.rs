use crate::model::domain::NormalizedCommand;
use crate::model::git_oid::is_non_zero_oid;
use crate::operations::git::cli_parser::parse_git_cli_args;

pub(crate) enum Target {
    Branch(String),
    Detached,
}

pub(crate) fn target(cmd: &NormalizedCommand) -> Option<Target> {
    if cmd.exit_code != 0 || cmd.raw_argv.is_empty() {
        return None;
    }
    let parsed = parse_git_cli_args(&super::normalized_args(&cmd.raw_argv));
    if parsed.command.as_deref() != Some("switch") {
        return None;
    }
    // A replacement tree can make a canonical tracked path remain untracked.
    // Require the command itself to disable replacements; later refs cannot
    // establish which object view Git used when it discarded the edits.
    let mut canonical_objects = false;
    let mut globals = parsed.global_args.iter();
    while let Some(arg) = globals.next() {
        match arg.as_str() {
            "--no-replace-objects" => canonical_objects = true,
            "-C" if globals.next().is_some() => {}
            value if value.starts_with("-C") && value.len() > 2 => {}
            _ => return None,
        }
    }
    if !canonical_objects {
        return None;
    }
    let mut force = false;
    let mut detached = false;
    let mut destination = None;
    for arg in &parsed.command_args {
        match arg.as_str() {
            "--discard-changes" | "--force" | "-f" => force = true,
            "--detach" | "-d" => detached = true,
            "--quiet" | "-q" | "--no-guess" => {}
            value if !value.starts_with('-') && destination.is_none() => {
                destination = Some(value);
            }
            _ => return None,
        }
    }
    let destination = destination.filter(|_| force)?;
    if detached {
        return is_non_zero_oid(destination).then_some(Target::Detached);
    }
    // Limit symbolic targets to literal branch names; revision expressions and
    // previous-branch selectors do not prove the new branch identity.
    if destination.is_empty()
        || destination.len() > 4096
        || !destination
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte))
        || destination.contains("..")
        || destination == "HEAD"
        || destination.starts_with("refs/") && !destination.starts_with("refs/heads/")
    {
        return None;
    }
    Some(Target::Branch(if destination.starts_with("refs/heads/") {
        destination.to_owned()
    } else {
        format!("refs/heads/{destination}")
    }))
}
