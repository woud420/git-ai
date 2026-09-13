mod pager;
mod render;
use crate::config::{Config, NotesBackendKind};
use crate::error::GitAiError;
use pager::{run_pager, stream_to_stdout};
use render::LogRenderer;
use std::io::{self, IsTerminal};
use std::process::ExitStatus;

const LOG_BATCH_SIZE: usize = 24;
const GIT_LOG_FIELD_COUNT: usize = 8;
const GIT_LOG_FORMAT: &str = "%H%x00%P%x00%an%x00%ae%x00%aI%x00%D%x00%s%x00%b%x00";

/// Extract recognized Git global flags from args so they can be placed
/// before repository discovery. Everything else is interpreted as a log arg.
///
/// We deliberately skip the ambiguous short forms `-p` (paginate vs patch),
/// `-P` (no-pager vs perl-regexp), `-C` (change-dir vs copy-detection),
/// and bare `-c` (config vs combined-diff).
/// Their long-form equivalents (`--paginate`, `--no-pager`, `--git-dir`,
/// `--work-tree`, `-c key=val`) are handled correctly.
fn extract_git_global_args(args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut global_args: Vec<String> = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        // `--` marks the end of options; everything after is a pathspec.
        if arg == "--" {
            rest.extend_from_slice(&args[i..]);
            break;
        }

        // --- Global no-value long options (unambiguous with git log) ---
        if matches!(
            arg.as_str(),
            "--paginate"
                | "--no-pager"
                | "--no-replace-objects"
                | "--no-lazy-fetch"
                | "--no-optional-locks"
                | "--no-advice"
                | "--bare"
                | "--literal-pathspecs"
                | "--glob-pathspecs"
                | "--noglob-pathspecs"
                | "--icase-pathspecs"
        ) {
            global_args.push(arg.clone());
            i += 1;
            continue;
        }

        // --- Global takes-value long options: --opt=val or --opt val ---
        if matches!(
            arg.as_str(),
            "--git-dir"
                | "--work-tree"
                | "--namespace"
                | "--config-env"
                | "--list-cmds"
                | "--attr-source"
                | "--super-prefix"
        ) {
            global_args.push(arg.clone());
            if i + 1 < args.len() {
                global_args.push(args[i + 1].clone());
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        // --exec-path can be standalone (query) or --exec-path=<path> (set)
        if arg == "--exec-path" {
            global_args.push(arg.clone());
            i += 1;
            continue;
        }

        // =<value> forms for all long takes-value options
        if arg.starts_with("--git-dir=")
            || arg.starts_with("--work-tree=")
            || arg.starts_with("--namespace=")
            || arg.starts_with("--config-env=")
            || arg.starts_with("--list-cmds=")
            || arg.starts_with("--attr-source=")
            || arg.starts_with("--super-prefix=")
            || arg.starts_with("--exec-path=")
        {
            global_args.push(arg.clone());
            i += 1;
            continue;
        }

        // -C is deliberately NOT extracted:
        //   git global: -C <path> (change directory before doing anything)
        //   git log:    -C (detect copies, no argument)
        // Since all args arrive after the `log` keyword is stripped, a bare
        // `-C` is far more likely to be copy-detection. Users needing the
        // global form should use `--git-dir` or `--work-tree` instead.

        // -c <key>=<value>: git config override.
        // Git config keys are always `section.variable=value`, so a valid
        // assignment contains a '.' before the first '='.  A bare `-c`
        // without such a token is git log's combined-diff flag, and a next
        // token like `--format=%H` is a log option (no dot in key portion).
        if arg == "-c"
            && i + 1 < args.len()
            && args[i + 1]
                .find('=')
                .is_some_and(|eq| args[i + 1][..eq].contains('.'))
        {
            global_args.push(arg.clone());
            global_args.push(args[i + 1].clone());
            i += 2;
            continue;
        }

        // -c<key>=<value> sticky form — apply same dot-check as the spaced form
        if arg.starts_with("-c")
            && arg.len() > 2
            && arg[2..]
                .find('=')
                .is_some_and(|eq| arg[2..2 + eq].contains('.'))
        {
            global_args.push(arg.clone());
            i += 1;
            continue;
        }

        // -p, -P, and -C are deliberately NOT extracted:
        //   -p = git log --patch (not --paginate)
        //   -P = git log --perl-regexp (not --no-pager)
        //   -C = git log copy-detection (not --git-dir/change-dir)

        // Everything else (including --help, --version, -h, -v, -p, -P, -C,
        // and all git-log options) remains a log arg.
        rest.push(arg.clone());
        i += 1;
    }

    (global_args, rest)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLogArgs {
    git_log_args: Vec<String>,
    plain: bool,
    show_raw_notes: bool,
    oneline: bool,
    show_decorations: bool,
    help: bool,
}

impl Default for ParsedLogArgs {
    fn default() -> Self {
        Self {
            git_log_args: Vec::new(),
            plain: false,
            show_raw_notes: false,
            oneline: false,
            show_decorations: true,
            help: false,
        }
    }
}

/// Handle the `git ai log` command.
///
/// Unlike the old implementation, this does not proxy to `git log --notes=ai`.
/// It streams commits from git with a stable machine-readable format, resolves
/// authorship notes through the configured notes backend, and renders Git AI
/// stats by default. Raw note content is shown only with `--raw` or `--notes`.
pub fn handle_log(args: &[String]) -> ExitStatus {
    match run_log(args) {
        Ok(status) => status,
        Err(LogError::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe => {
            status_from_code(0)
        }
        Err(error) => {
            eprintln!("git-ai log: {}", error);
            status_from_code(1)
        }
    }
}

fn run_log(args: &[String]) -> Result<ExitStatus, LogError> {
    let (global_args, log_args) = extract_git_global_args(args);
    let parsed = parse_log_args(&log_args).map_err(LogError::Message)?;

    if parsed.help {
        print_log_help();
        return Ok(status_from_code(0));
    }

    if parsed.plain {
        return run_plain_log(&global_args, &parsed.git_log_args);
    }

    let repository_global_args = repository_global_args(&global_args);
    let repo = crate::operations::git::repository::find_repository(&repository_global_args)
        .map_err(LogError::Git)?;
    let use_pager = should_use_pager(&global_args);
    let renderer = LogRenderer::new(repo, parsed)?;

    if use_pager {
        run_pager(renderer)?;
    } else {
        stream_to_stdout(renderer)?;
    }
    Ok(status_from_code(0))
}

fn run_plain_log(global_args: &[String], git_log_args: &[String]) -> Result<ExitStatus, LogError> {
    if Config::get().notes_backend_kind() != NotesBackendKind::GitNotes {
        return Err(LogError::Message(
            "plain git log --notes=ai only supports the git_notes backend".to_string(),
        ));
    }

    let mut command_args = global_args.to_vec();
    command_args.push("log".to_string());
    command_args.push("--notes=ai".to_string());
    command_args.extend(git_log_args.iter().cloned());

    let mut child = crate::clients::git_cli::spawn_git_passthrough(&command_args)?;
    child.wait().map_err(LogError::Io)
}

fn repository_global_args(global_args: &[String]) -> Vec<String> {
    global_args
        .iter()
        .filter(|arg| !matches!(arg.as_str(), "--paginate" | "--no-pager"))
        .cloned()
        .collect()
}

fn should_use_pager(global_args: &[String]) -> bool {
    if global_args.iter().any(|arg| arg == "--no-pager") {
        return false;
    }
    let forced = global_args.iter().any(|arg| arg == "--paginate");
    forced || std::io::stdout().is_terminal()
}

fn parse_log_args(args: &[String]) -> Result<ParsedLogArgs, String> {
    let mut parsed = ParsedLogArgs::default();
    let mut passthrough = Vec::new();
    let mut after_double_dash = false;
    let plain_requested = contains_plain_flag(args);

    for arg in args {
        if after_double_dash {
            passthrough.push(arg.clone());
            continue;
        }

        if arg == "--" {
            after_double_dash = true;
            passthrough.push(arg.clone());
            continue;
        }

        if arg == "--plain" {
            parsed.plain = true;
            continue;
        }

        if plain_requested {
            passthrough.push(arg.clone());
            continue;
        }

        match arg.as_str() {
            "--help" | "-h" => {
                parsed.help = true;
            }
            "--raw" | "--notes" | "--show-notes" => {
                parsed.show_raw_notes = true;
            }
            "--oneline" => {
                parsed.oneline = true;
            }
            "--decorate" | "--decorate=short" | "--decorate=full" | "--decorate=auto" => {
                parsed.show_decorations = true;
            }
            "--no-decorate" => {
                parsed.show_decorations = false;
            }
            _ if is_unsupported_render_arg(arg) => {
                return Err(format!(
                    "unsupported git log rendering option '{}'. `git-ai log` owns rendering so it can show authorship stats; use plain `git log` for this option.",
                    arg
                ));
            }
            _ => passthrough.push(arg.clone()),
        }
    }

    parsed.git_log_args = passthrough;
    Ok(parsed)
}

fn contains_plain_flag(args: &[String]) -> bool {
    args.iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--plain")
}

fn is_unsupported_render_arg(arg: &str) -> bool {
    matches!(
        arg,
        "--format"
            | "--pretty"
            | "--graph"
            | "--patch"
            | "-p"
            | "--stat"
            | "--shortstat"
            | "--numstat"
            | "--name-only"
            | "--name-status"
            | "--check"
            | "--summary"
            | "--show-signature"
            | "--cc"
            | "-c"
    ) || arg.starts_with("--format=")
        || arg.starts_with("--pretty=")
        || arg.starts_with("--notes=")
        || arg.starts_with("--stat=")
        || arg.starts_with("--patch=")
}

fn print_log_help() {
    println!("Usage: git-ai log [--raw|--notes] [--plain] [git log filters] [--] [pathspecs...]");
    println!();
    println!("Shows commit history with Git AI authorship stats.");
    println!();
    println!("Options:");
    println!("  --raw, --notes    Include raw authorship note data after the stats");
    println!("  --show-notes      Alias for --notes");
    println!("  --plain           Run git log --notes=ai directly (git_notes backend only)");
    println!("  --oneline         Compact commit header");
    println!("  --no-decorate     Hide ref decorations");
    println!("  --no-pager        Stream output instead of opening the pager");
    println!();
    println!("Common git log filters such as -n, --max-count, --author, --grep,");
    println!("--since, --until, revisions, and pathspecs are passed through to git.");
}

#[derive(Debug)]
enum LogError {
    Git(GitAiError),
    Io(io::Error),
    Message(String),
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogError::Git(error) => write!(f, "{}", error),
            LogError::Io(error) => write!(f, "{}", error),
            LogError::Message(message) => write!(f, "{}", message),
        }
    }
}

impl From<GitAiError> for LogError {
    fn from(value: GitAiError) -> Self {
        LogError::Git(value)
    }
}

impl From<io::Error> for LogError {
    fn from(value: io::Error) -> Self {
        LogError::Io(value)
    }
}

#[cfg(unix)]
fn status_from_code(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
fn status_from_code(code: i32) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(code as u32)
}

#[cfg(test)]
mod tests;
