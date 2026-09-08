//! Integration tests for AI provenance tracking via bash tool pre/post snapshots.
//!
//! Each test simulates what happens when an AI coding agent executes a bash
//! command: the system takes a pre-snapshot of filesystem metadata, the bash
//! command runs, and then a post-snapshot detects which files changed. This
//! validates that the stat-diff mechanism correctly identifies created,
//! modified, and deleted files across a wide variety of real-world shell
//! commands.

use crate::bash_tool_common::{add_and_commit, post_hook, pre_hook, repo_root};
use crate::repos::test_repo::TestRepo;
#[cfg(unix)]
use crate::repos::write_executable_script;
use git_ai::operations::commands::checkpoint_agent::bash_tool::{
    BashCheckpointAction, BashPostHookResult, diff, git_status_fallback, snapshot,
};
#[cfg(unix)]
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run a bash command in the repo and assert it succeeds.
fn run_bash(repo: &TestRepo, program: &str, args: &[&str]) -> std::process::Output {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo.path())
        .output()
        .unwrap_or_else(|e| panic!("{} {:?} failed to start: {}", program, args, e));
    assert!(
        output.status.success(),
        "{} {:?} failed: {}",
        program,
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Assert that a BashCheckpointAction::Checkpoint contains the expected path.
fn assert_checkpoint_contains(result: &BashPostHookResult, expected_path: &str) {
    match &result.action {
        BashCheckpointAction::Checkpoint(paths) => {
            assert!(
                paths.iter().any(|p| p.contains(expected_path)),
                "Expected checkpoint to contain '{}'; got {:?}",
                expected_path,
                paths
            );
        }
        BashCheckpointAction::NoChanges => {
            panic!(
                "Expected Checkpoint containing '{}', got NoChanges",
                expected_path
            );
        }
        other => {
            panic!("Expected Checkpoint, got {:?}", other);
        }
    }
}

/// Assert that a BashCheckpointAction::Checkpoint does NOT contain a path.
fn assert_checkpoint_excludes(result: &BashPostHookResult, excluded_path: &str) {
    if let BashCheckpointAction::Checkpoint(paths) = &result.action {
        assert!(
            !paths.iter().any(|p| p.contains(excluded_path)),
            "Expected checkpoint NOT to contain '{}'; got {:?}",
            excluded_path,
            paths
        );
    }
}

/// Assert that a BashCheckpointAction is NoChanges.
fn assert_no_changes(result: &BashPostHookResult) {
    match &result.action {
        BashCheckpointAction::NoChanges => {}
        other => {
            panic!("Expected NoChanges, got {:?}", other);
        }
    }
}

/// Get the checkpoint paths from an action, panicking if not a Checkpoint.
fn checkpoint_paths(result: &BashPostHookResult) -> &[String] {
    match &result.action {
        BashCheckpointAction::Checkpoint(paths) => paths,
        other => panic!("Expected Checkpoint, got {:?}", other),
    }
}

// ===========================================================================
// Category 14: Pre-commit hook formatter attribution
//
// Verifies that when a git commit runs inside an AI agent's bash tool call,
// and git's pre-commit hook runs a formatter (or any tool that modifies files),
// those changes are properly detected by the stat-diff mechanism and attributed
// to the AI agent.
// ===========================================================================

/// Install a git pre-commit hook script in the test repo.
/// The hook must be executable and located at `.git/hooks/pre-commit`.
#[cfg(unix)]
fn install_pre_commit_hook(repo: &TestRepo, script: &str) {
    let git_dir = repo.path().join(".git");
    // For linked worktrees, .git is a file pointing to the real git dir
    let hooks_dir = if git_dir.is_file() {
        let content = fs::read_to_string(&git_dir).expect("read .git file");
        let real_git_dir = content
            .trim()
            .strip_prefix("gitdir: ")
            .expect("parse gitdir");
        std::path::PathBuf::from(real_git_dir).join("hooks")
    } else {
        git_dir.join("hooks")
    };
    fs::create_dir_all(&hooks_dir).expect("create hooks dir");
    let hook_path = hooks_dir.join("pre-commit");
    write_executable_script(&hook_path, script).expect("write pre-commit hook");
}

/// Run a raw git command in the repo (without bypassing hooks).
/// Unlike `run_bash`, this returns the full output including exit status
/// without asserting success, so we can check for hook failures.
#[cfg(unix)]
fn run_git_with_hooks(repo: &TestRepo, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {:?} failed to start: {}", args, e))
}

mod bulk_changes;
mod file_operations;
mod filesystem_edges;
mod precommit_hooks;
mod read_only;
mod status_fallback;
