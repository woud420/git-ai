//! Tests for cross-repo CWD attribution.
//!
//! These tests verify that when the CWD of the agent (hook call) differs from the
//! repo root where files are being edited, attribution is still correctly found.
//!
//! Scenarios covered:
//! 1. CWD != repo root, single repo edit
//! 2. CWD != repo root, edits in several different repos
//! 3. CWD != repo root, edits in several repos + CWD repo itself
//! 4. CWD is a parent directory above all repos (e.g. ~/projects)
//! 5. CWD is a parent directory above all repos, edits in several repo subpaths
//! 6. Agent preset (e.g. Claude) with CWD in repo A editing files in repo B (issue #871)

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::fixture_path;

use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Creates a unique temporary directory for tests
fn create_unique_workspace(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let base = std::env::temp_dir();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir_name = format!("{}-{}-{}-{}", prefix, now, pid, seq);
    let path = base.join(dir_name);
    fs::create_dir_all(&path).expect("failed to create workspace dir");
    path
}

mod blame_and_agent_context;
mod cwd_resolution;
mod nested_repositories;
mod non_repository_cwd;
