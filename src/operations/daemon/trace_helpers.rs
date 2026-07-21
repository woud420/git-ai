use crate::operations::git::cli_parser::parse_git_cli_args;
use crate::operations::git::repo_state::worktree_root_for_path;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub fn is_trace_payload(payload: &Value) -> bool {
    payload.get("event").and_then(Value::as_str).is_some()
}

pub fn trace_root_sid(sid: &str) -> &str {
    sid.split('/').next().unwrap_or(sid)
}

pub fn is_terminal_root_trace_event(event: &str, sid: &str, root: &str) -> bool {
    sid == root && event == "atexit"
}

pub fn daemon_worktree_from_repo_path(repo_path: &Path) -> Option<PathBuf> {
    if repo_path.file_name().and_then(|name| name.to_str()) == Some(".git") {
        return repo_path.parent().map(PathBuf::from);
    }

    let linked_gitdir_file = repo_path.join("gitdir");
    if linked_gitdir_file.is_file() {
        let content = fs::read_to_string(&linked_gitdir_file).ok()?;
        let linked = PathBuf::from(content.trim());
        if linked.file_name().and_then(|name| name.to_str()) == Some(".git") {
            return linked.parent().map(PathBuf::from);
        }
    }

    None
}

pub fn trace_payload_worktree_hint(payload: &Value) -> Option<PathBuf> {
    let normalize = |path: PathBuf| worktree_root_for_path(&path).unwrap_or(path);
    let argv = trace_payload_argv(payload);
    let event = payload
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if event == "def_repo" {
        if let Some(path) = payload
            .get("worktree")
            .or_else(|| payload.get("repo_working_dir"))
            .and_then(Value::as_str)
        {
            return Some(normalize(PathBuf::from(path)));
        }
        if let Some(repo_path) = payload.get("repo").and_then(Value::as_str) {
            let candidate = PathBuf::from(repo_path);
            if let Some(worktree) = daemon_worktree_from_repo_path(&candidate) {
                return Some(normalize(worktree));
            }
        }
    }
    if let Some(path) = payload.get("worktree").and_then(Value::as_str) {
        return Some(normalize(PathBuf::from(path)));
    }
    if let Some(path) = payload
        .get(super::TRACE_ROOT_WORKTREE_FIELD)
        .and_then(Value::as_str)
    {
        return Some(normalize(PathBuf::from(path)));
    }
    if let Some(cwd) = payload.get("cwd").and_then(Value::as_str)
        && let Some(base_dir) = trace_payload_command_base_dir(payload, &argv, Path::new(cwd))
    {
        return Some(normalize(base_dir));
    }
    let parsed = parse_git_cli_args(trace_invocation_args(&argv));
    let mut idx = 0usize;
    while idx < parsed.global_args.len() {
        let token = &parsed.global_args[idx];
        if token == "-C" {
            let path_arg = parsed.global_args.get(idx + 1)?;
            let candidate = PathBuf::from(path_arg);
            if candidate.is_absolute() {
                return Some(normalize(candidate));
            }
            return None;
        }
        if let Some(path_arg) = token.strip_prefix("-C")
            && !path_arg.is_empty()
        {
            let candidate = PathBuf::from(path_arg);
            if candidate.is_absolute() {
                return Some(normalize(candidate));
            }
            return None;
        }
        idx += 1;
    }
    if argv.is_empty() {
        return None;
    }
    None
}

pub fn trace_payload_command_base_dir(
    _payload: &Value,
    argv: &[String],
    cwd: &Path,
) -> Option<PathBuf> {
    let parsed = parse_git_cli_args(trace_invocation_args(argv));
    let mut base = cwd.to_path_buf();
    let mut idx = 0usize;

    while idx < parsed.global_args.len() {
        let token = &parsed.global_args[idx];

        if token == "-C" {
            let path_arg = parsed.global_args.get(idx + 1)?;
            let next_base = PathBuf::from(path_arg);
            base = if next_base.is_absolute() {
                next_base
            } else {
                base.join(next_base)
            };
            idx += 2;
            continue;
        }

        if let Some(path_arg) = token.strip_prefix("-C") {
            let next_base = PathBuf::from(path_arg);
            base = if next_base.is_absolute() {
                next_base
            } else {
                base.join(next_base)
            };
            idx += 1;
            continue;
        }

        idx += 1;
    }

    Some(base)
}

pub fn trace_payload_time_ns(payload: &Value) -> Option<u128> {
    payload
        .get("time")
        .and_then(Value::as_str)
        .and_then(super::rfc3339_to_unix_nanos)
        .or_else(|| {
            payload
                .get("time_ns")
                .and_then(Value::as_u64)
                .map(u128::from)
        })
        .or_else(|| payload.get("ts").and_then(Value::as_u64).map(u128::from))
        .or_else(|| {
            payload
                .get("t_abs")
                .and_then(Value::as_f64)
                .and_then(|seconds| {
                    if seconds.is_sign_negative() {
                        None
                    } else {
                        Some((seconds * 1_000_000_000_f64) as u128)
                    }
                })
        })
}

pub fn trace_payload_cmd_name(payload: &Value) -> Option<String> {
    payload
        .get("name")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

pub fn trace_payload_argv(payload: &Value) -> Vec<String> {
    payload
        .get("argv")
        .and_then(Value::as_array)
        .map(|argv| {
            argv.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub fn trace_payload_effective_argv(payload: &Value) -> Vec<String> {
    let argv = trace_payload_argv(payload);
    if !argv.is_empty() {
        return argv;
    }
    payload
        .get(super::TRACE_ROOT_ARGV_FIELD)
        .and_then(Value::as_array)
        .map(|argv| {
            argv.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub fn trace_payload_primary_command(payload: &Value) -> Option<String> {
    trace_payload_cmd_name(payload).or_else(|| {
        let argv = trace_payload_argv(payload);
        trace_argv_primary_command(&argv)
    })
}

pub fn trace_payload_root_started_at_ns(payload: &Value) -> Option<u128> {
    payload
        .get(super::TRACE_ROOT_STARTED_AT_NS_FIELD)
        .and_then(Value::as_u64)
        .map(u128::from)
}

pub fn trace_argv_primary_command(argv: &[String]) -> Option<String> {
    let mut idx = 0;
    if argv
        .first()
        .map(|token| {
            let file_name = Path::new(token)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(token);
            file_name == "git" || file_name == "git.exe"
        })
        .unwrap_or(false)
    {
        idx = 1;
    }
    while idx < argv.len() {
        let token = argv[idx].as_str();
        if token == "-C" {
            idx += 2;
            continue;
        }
        if matches!(
            token,
            "-c" | "--config-env"
                | "--git-dir"
                | "--work-tree"
                | "--namespace"
                | "--super-prefix"
                | "--exec-path"
                | "--worktree-attributes"
                | "--attr-source"
        ) {
            idx += 2;
            continue;
        }
        if token.starts_with("--") && token.contains('=') {
            idx += 1;
            continue;
        }
        if token.starts_with('-') {
            idx += 1;
            continue;
        }
        return Some(token.to_string());
    }
    None
}

/// Returns true when the trace2 event's command+argument pair is
/// guaranteed to never mutate repository state.
///
/// This extends the simple command check to handle mixed read/write commands
/// such as `branch`, `remote`, `stash`, `tag`, and `worktree`.
pub fn trace_invocation_is_definitely_read_only(
    primary_command: Option<&str>,
    argv: &[String],
) -> bool {
    use crate::operations::git::command_classification::is_definitely_read_only_git_invocation;
    match primary_command {
        Some(cmd) => is_definitely_read_only_git_invocation(
            cmd,
            &trace_invocation_command_args(Some(cmd), argv),
        ),
        None => false,
    }
}

pub fn trace_invocation_may_mutate_refs(primary_command: Option<&str>, argv: &[String]) -> bool {
    primary_command.is_some_and(|cmd| {
        crate::operations::git::command_classification::git_invocation_may_mutate_repo_state(
            cmd,
            &trace_invocation_command_args(Some(cmd), argv),
        )
    })
}

pub fn trace_command_uses_target_repo_context_only(primary_command: Option<&str>) -> bool {
    matches!(primary_command, Some("clone" | "init"))
}

pub fn trace_invocation_args(argv: &[String]) -> &[String] {
    if argv
        .first()
        .map(|token| {
            Path::new(token)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "git" || name == "git.exe")
        })
        .unwrap_or(false)
    {
        &argv[1..]
    } else {
        argv
    }
}

pub fn trace_invocation_command_args(
    primary_command: Option<&str>,
    argv: &[String],
) -> Vec<String> {
    let invocation = trace_invocation_args(argv);
    let parsed = parse_git_cli_args(invocation);
    if parsed.command.as_deref() == primary_command {
        return parsed.command_args;
    }

    let Some(primary) = primary_command else {
        return Vec::new();
    };
    invocation
        .iter()
        .position(|arg| arg == primary)
        .and_then(|idx| invocation.get(idx + 1..))
        .map(|args| args.to_vec())
        .unwrap_or_default()
}
