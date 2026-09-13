#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use git_ai::model::repository::notes_db::NotesDatabase;
use git_ai::notes::reference_server::ReferenceServer;
use repos::test_repo::{DaemonTestScope, TestRepo, real_git_executable};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_path(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{seq}", std::process::id()))
}

fn run_git(args: &[&str]) -> String {
    let output = Command::new(real_git_executable())
        .args(args)
        .output()
        .expect("git command should execute");

    assert!(
        output.status.success(),
        "git {} failed:\nstdout: {}\nstderr: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn read_note_from_worktree(repo_path: &Path, commit_sha: &str) -> Option<String> {
    repos::test_repo::TestRepo::new_at_path(repo_path).read_authorship_note(commit_sha)
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_string(),
            Err(_) => "unknown panic payload".to_string(),
        },
    }
}

#[path = "notes_sync_regression/http_cache.rs"]
mod http_cache;

#[path = "notes_sync_regression/clone_and_fetch.rs"]
mod clone_and_fetch;
#[path = "notes_sync_regression/clone_paths.rs"]
mod clone_paths;
#[path = "notes_sync_regression/pull_notes.rs"]
mod pull_notes;
#[path = "notes_sync_regression/push_notes.rs"]
mod push_notes;
