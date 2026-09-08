#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseArgsSummary {
    pub is_control_mode: bool,
    pub has_root: bool,
    pub onto_spec: Option<String>,
    pub positionals: Vec<String>,
}

pub fn summarize_rebase_args(command_args: &[String]) -> RebaseArgsSummary {
    for mode in [
        "--continue",
        "--abort",
        "--skip",
        "--quit",
        "--show-current-patch",
    ] {
        if command_args.iter().any(|arg| arg == mode) {
            return RebaseArgsSummary {
                is_control_mode: true,
                has_root: false,
                onto_spec: None,
                positionals: Vec::new(),
            };
        }
    }

    let mut has_root = false;
    let mut onto_spec: Option<String> = None;
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 0usize;

    while i < command_args.len() {
        let arg = command_args[i].as_str();

        if arg == "--" {
            break;
        }

        if arg == "--onto" {
            if let Some(next) = command_args.get(i + 1) {
                onto_spec = Some(next.clone());
                i += 2;
                continue;
            }
            break;
        }
        if let Some(spec) = arg.strip_prefix("--onto=") {
            onto_spec = Some(spec.to_string());
            i += 1;
            continue;
        }

        if arg == "--root" {
            has_root = true;
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            let takes_value = matches!(
                arg,
                "-s" | "--strategy"
                    | "-X"
                    | "--strategy-option"
                    | "-x"
                    | "--exec"
                    | "--empty"
                    | "-C"
                    | "-S"
                    | "--gpg-sign"
            );
            if takes_value && !arg.contains('=') {
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        positionals.push(arg.to_string());
        i += 1;
    }

    RebaseArgsSummary {
        is_control_mode: false,
        has_root,
        onto_spec,
        positionals,
    }
}

pub fn rebase_has_control_mode(command_args: &[String]) -> bool {
    summarize_rebase_args(command_args).is_control_mode
}

pub fn explicit_rebase_branch_arg(command_args: &[String]) -> Option<String> {
    let summary = summarize_rebase_args(command_args);
    if summary.is_control_mode {
        return None;
    }

    if summary.has_root {
        summary.positionals.first().cloned()
    } else {
        summary.positionals.get(1).cloned()
    }
}

pub fn stash_subcommand(command_args: &[String]) -> Option<&str> {
    match command_args.first().map(String::as_str) {
        Some("push" | "save" | "apply" | "pop" | "drop" | "list" | "branch" | "show") => {
            command_args.first().map(String::as_str)
        }
        _ => None,
    }
}

pub fn stash_requires_target_resolution(command_args: &[String]) -> bool {
    matches!(
        stash_subcommand(command_args),
        Some("apply" | "pop" | "drop" | "branch")
    )
}

pub fn stash_target_spec(command_args: &[String]) -> Option<&str> {
    if !stash_requires_target_resolution(command_args) {
        return None;
    }

    // For "branch", the format is: git stash branch <branchname> [<stash>]
    // The stash ref is the second positional arg (after the branch name).
    let is_branch = stash_subcommand(command_args) == Some("branch");

    let remaining = command_args.get(1..)?;
    let mut saw_separator = false;
    let mut positional_count = 0u32;
    for arg in remaining {
        if arg == "--" {
            saw_separator = true;
            continue;
        }
        if !saw_separator && arg.starts_with('-') {
            continue;
        }
        positional_count += 1;
        // For "branch", skip the first positional (branch name) and return the second (stash ref).
        // For other subcommands, return the first positional (stash ref).
        if is_branch && positional_count == 1 {
            continue;
        }
        return Some(arg.as_str());
    }

    None
}
