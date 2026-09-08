//! Tests for multi-repository workspace support.
//!
//! This test module verifies that git-ai correctly handles workspaces that contain
//! multiple independent git repositories. The main scenarios tested are:
//!
//! 1. Detecting git repository from file paths when workspace root isn't a git repo
//! 2. Grouping files by their containing repository
//! 3. Handling submodules correctly (should be ignored in favor of parent repo)
//! 4. Edge cases with nested git directories
//! 5. Cross-repo checkpoints: AI edits from one repo to files in another repo

use crate::repos::test_repo::TestRepo;

use git_ai::error::GitAiError;
use git_ai::operations::git::repository::{
    find_repository_for_file, find_repository_in_path, group_files_by_repository,
};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// Creates a unique temporary directory for tests
fn create_unique_tmp_dir(prefix: &str) -> Result<PathBuf, GitAiError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let base = std::env::temp_dir();

    for _attempt in 0..100u32 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir_name = format!("{}-{}-{}-{}", prefix, now, pid, seq);
        let path = base.join(dir_name);

        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(GitAiError::IoError(e)),
        }
    }

    Err(GitAiError::Generic(
        "Failed to create a unique temporary directory".to_string(),
    ))
}

/// Initializes a git repository at the given path
fn init_git_repo(path: &PathBuf) -> Result<(), GitAiError> {
    fs::create_dir_all(path)?;

    let output = Command::new("git")
        .current_dir(path)
        .args(["init"])
        .output()
        .map_err(|e| GitAiError::Generic(format!("Failed to run git init: {}", e)))?;

    if !output.status.success() {
        return Err(GitAiError::Generic(format!(
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    // Configure user for the repository
    Command::new("git")
        .current_dir(path)
        .args(["config", "user.name", "Test User"])
        .output()
        .ok();

    Command::new("git")
        .current_dir(path)
        .args(["config", "user.email", "test@example.com"])
        .output()
        .ok();

    Ok(())
}

/// Creates a file at the given path with the specified content
fn create_file(path: &PathBuf, content: &str) -> Result<(), GitAiError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

/// Clean up a temporary directory
fn cleanup_tmp_dir(path: &PathBuf) {
    let _ = fs::remove_dir_all(path);
}

mod cross_repo_checkpoints;
mod file_grouping;
mod repository_discovery;
