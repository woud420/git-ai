//! Shared "resolve or exit" helpers for `git-ai` command handlers: repository
//! discovery that prints the standard failure message and exits, plus a
//! generic `"{what} failed: {e}"` + exit(1) helper for arms that already use
//! that exact message shape.
//!
//! Never use `fail` for the checkpoint/hook-input paths — those deliberately
//! exit(0) on failure so a git-ai error never fails an agent's edit.

use crate::operations::git::find_repository;
use crate::operations::git::find_repository_in_path;
use crate::operations::git::repository::Repository;
use std::env;
use std::fmt::Display;

/// Resolve the repository via git's normal discovery (no explicit cwd),
/// printing "Failed to find repository: {e}" and exiting with status 1 on
/// failure.
pub(crate) fn resolve_repo_or_fail() -> Repository {
    match find_repository(&Vec::<String>::new()) {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("Failed to find repository: {}", e);
            std::process::exit(1);
        }
    }
}

/// Resolve the repository starting from the current working directory,
/// returning both the repository and the cwd string used to discover it —
/// some callers need it again afterward (e.g. to resolve a relative file
/// path argument). Same failure behavior as `resolve_repo_or_fail`.
pub(crate) fn resolve_repo_in_cwd_or_fail() -> (Repository, String) {
    let current_dir = env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .to_string_lossy()
        .to_string();
    match find_repository_in_path(&current_dir) {
        Ok(repo) => (repo, current_dir),
        Err(e) => {
            eprintln!("Failed to find repository: {}", e);
            std::process::exit(1);
        }
    }
}

/// Print `"{what} failed: {e}"` to stderr and exit with status 1. Only use
/// this where the existing message already matches that exact template.
pub(crate) fn fail(what: &str, e: impl Display) -> ! {
    eprintln!("{what} failed: {e}");
    std::process::exit(1);
}
