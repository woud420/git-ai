#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Get the path to a test fixture file
///
/// # Example
/// ```no_run
/// use test_utils::fixture_path;
///
/// let path = fixture_path("example.json");
/// // Returns: /path/to/project/tests/fixtures/example.json
/// ```
pub fn fixture_path(filename: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/")).join(filename)
}

/// Load the contents of a test fixture file as a string
///
/// # Example
/// ```no_run
/// use test_utils::load_fixture;
///
/// let contents = load_fixture("example.json");
/// // Returns the string contents of tests/fixtures/example.json
/// ```
///
/// # Panics
/// Panics if the fixture file cannot be read
pub fn load_fixture(filename: &str) -> String {
    std::fs::read_to_string(fixture_path(filename))
        .unwrap_or_else(|_| panic!("Failed to read fixture: {}", filename))
}

/// Get the path to a transcript fixture file under `tests/transcripts/fixtures/`.
/// Distinct root from [`fixture_path`] (`tests/fixtures/`) — do not merge.
pub fn transcript_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("transcripts")
        .join("fixtures")
        .join(name)
}

/// Extract the outermost `{ ... }` JSON object from mixed CLI output by
/// locating the first `{` and the last `}`. Does not validate nesting; used
/// to strip surrounding log/diagnostic text before `serde_json::from_str`.
pub fn extract_json_object(output: &str) -> String {
    let start = output.find('{').unwrap_or(0);
    let end = output.rfind('}').unwrap_or(output.len().saturating_sub(1));
    output[start..=end].to_string()
}

/// Run a raw `git` command against `cwd` and assert it succeeds. Deliberately
/// unsynced/unisolated (no HOME override, no daemon sync) — different from
/// `TestRepo::git`, which wires up test isolation and daemon syncing.
pub fn raw_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed:\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Create a fresh temp dir holding an isolated bash-history sqlite db path.
/// The returned `TempDir` must be kept alive for the duration of the test.
pub fn isolated_bash_history_db_path() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("failed to create isolated bash history db dir");
    let path = dir.path().join("bash-history.db");
    (dir, path.to_string_lossy().to_string())
}
