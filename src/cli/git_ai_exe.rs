//! Resolution of the current `git-ai` executable and spawning of internal
//! (background) `git-ai` subcommands from within a running command.

use crate::error::GitAiError;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn resolve_git_ai_exe_from_invocation_path(path: PathBuf) -> PathBuf {
    let canonical_path = crate::operations::git::canonicalize::canonicalize_or_self(&path);

    // Get platform-specific executable names
    let git_name = if cfg!(windows) { "git.exe" } else { "git" };
    let git_ai_name = if cfg!(windows) {
        "git-ai.exe"
    } else {
        "git-ai"
    };

    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return canonical_path;
    };

    if file_name == git_name {
        let sibling = path.with_file_name(git_ai_name);
        if sibling.exists() {
            return sibling;
        }

        let canonical_sibling = canonical_path.with_file_name(git_ai_name);
        if canonical_sibling.exists() {
            return canonical_sibling;
        }

        return PathBuf::from(git_ai_name);
    }

    let hook_candidate = file_name.strip_suffix(".exe").unwrap_or(file_name);
    if crate::operations::commands::git_hook_handlers::is_git_hook_binary_name(hook_candidate) {
        if canonical_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name == git_ai_name)
        {
            return canonical_path;
        }

        let sibling = path.with_file_name(git_ai_name);
        if sibling.exists() {
            return sibling;
        }

        let canonical_sibling = canonical_path.with_file_name(git_ai_name);
        if canonical_sibling.exists() {
            return canonical_sibling;
        }

        return PathBuf::from(git_ai_name);
    }

    canonical_path
}

pub(crate) fn current_git_ai_exe() -> Result<PathBuf, GitAiError> {
    let path = std::env::current_exe()?;
    Ok(resolve_git_ai_exe_from_invocation_path(path))
}

fn internal_git_ai_command_with_exe(exe: PathBuf, subcommand: &str) -> Command {
    let mut cmd = Command::new(exe);
    cmd.arg(subcommand).env(
        crate::operations::commands::git_hook_handlers::ENV_SKIP_ALL_HOOKS,
        "1",
    );
    cmd
}

pub fn spawn_internal_git_ai_subcommand(
    subcommand: &str,
    extra_args: &[&str],
    guard_env: &str,
    extra_env: &[(&str, &str)],
) -> bool {
    if guard_env.is_empty() || std::env::var(guard_env).as_deref() == Ok("1") {
        return false;
    }

    let Ok(exe) = current_git_ai_exe() else {
        return false;
    };
    let mut cmd = internal_git_ai_command_with_exe(exe, subcommand);
    cmd.args(extra_args);

    cmd.env(guard_env, "1");
    for (key, value) in extra_env {
        cmd.env(key, value);
    }

    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_git_ai_exe_from_git_sibling_prefers_git_ai() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir
            .path()
            .join(if cfg!(windows) { "git.exe" } else { "git" });
        let git_ai = dir.path().join(if cfg!(windows) {
            "git-ai.exe"
        } else {
            "git-ai"
        });
        std::fs::write(&git, "").unwrap();
        std::fs::write(&git_ai, "").unwrap();

        let resolved = resolve_git_ai_exe_from_invocation_path(git);
        assert_eq!(resolved, git_ai);
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_git_ai_exe_from_hook_symlink_uses_canonical_git_ai() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let git_ai = dir.path().join("git-ai");
        let hook = dir.path().join("pre-push");
        std::fs::write(&git_ai, "").unwrap();
        symlink(&git_ai, &hook).unwrap();

        let resolved = resolve_git_ai_exe_from_invocation_path(hook);
        assert_eq!(
            std::fs::canonicalize(resolved).unwrap(),
            std::fs::canonicalize(git_ai).unwrap()
        );
    }

    #[test]
    fn test_resolve_git_ai_exe_from_hook_without_target_falls_back_to_git_ai_name() {
        let dir = tempfile::tempdir().unwrap();
        let hook = dir.path().join(if cfg!(windows) {
            "pre-push.exe"
        } else {
            "pre-push"
        });
        std::fs::write(&hook, "").unwrap();

        let resolved = resolve_git_ai_exe_from_invocation_path(hook);
        let expected = if cfg!(windows) {
            "git-ai.exe"
        } else {
            "git-ai"
        };
        assert_eq!(resolved, PathBuf::from(expected));
    }

    #[cfg(windows)]
    #[test]
    fn test_resolve_git_ai_exe_from_hook_exe_name_falls_back_to_git_ai_exe() {
        let dir = tempfile::tempdir().unwrap();
        let hook = dir.path().join("pre-push.exe");
        std::fs::write(&hook, "").unwrap();

        let resolved = resolve_git_ai_exe_from_invocation_path(hook);
        assert_eq!(resolved, PathBuf::from("git-ai.exe"));
    }

    #[test]
    fn test_internal_git_ai_command_sets_skip_all_hooks_env() {
        let exe = PathBuf::from("/tmp/git-ai-test");
        let cmd = internal_git_ai_command_with_exe(exe.clone(), "status");

        assert_eq!(cmd.get_program(), exe.as_os_str());
        assert_eq!(
            cmd.get_args().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new("status")]
        );
        assert!(
            cmd.get_envs().any(|(k, v)| {
                k == std::ffi::OsStr::new(
                    crate::operations::commands::git_hook_handlers::ENV_SKIP_ALL_HOOKS,
                ) && v == Some(std::ffi::OsStr::new("1"))
            }),
            "internal command must always set GIT_AI_SKIP_ALL_HOOKS=1"
        );
    }

    #[test]
    fn test_spawn_internal_git_ai_subcommand_respects_guard_env() {
        let key = "GIT_AI_TEST_WORKER_GUARD";
        unsafe {
            std::env::set_var(key, "1");
        }
        let spawned = spawn_internal_git_ai_subcommand("status", &[], key, &[]);
        unsafe {
            std::env::remove_var(key);
        }
        assert!(
            !spawned,
            "spawn should be skipped when guard env is already set"
        );
    }

    #[test]
    fn test_spawn_internal_git_ai_subcommand_requires_non_empty_guard_env() {
        let spawned = spawn_internal_git_ai_subcommand("status", &[], "", &[]);
        assert!(!spawned, "spawn should be skipped when guard env is empty");
    }

    #[test]
    fn test_current_git_ai_exe_returns_path() {
        // Should return a path (either current exe or git-ai)
        let result = current_git_ai_exe();
        assert!(result.is_ok(), "current_git_ai_exe should not fail");
        let path = result.unwrap();
        assert!(!path.as_os_str().is_empty(), "path should not be empty");
    }
}
