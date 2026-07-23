//! Path normalization, git-dir helpers, watermark coverage checks, and
//! gitignore-based file filtering for the bash-tool stat-diff system.

use crate::error::GitAiError;
use crate::operations::authorship::ignore::{
    default_ignore_patterns, load_git_ai_ignore_patterns_from_path,
    load_linguist_generated_patterns_from_path,
};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::MTIME_GRACE_WINDOW_NS;
use super::snapshot::system_time_to_nanos;

// ---------------------------------------------------------------------------
// Path normalization
// ---------------------------------------------------------------------------

/// Normalize a path for use as HashMap key.
/// On case-insensitive filesystems (macOS, Windows), case-fold to lowercase.
pub fn normalize_path(p: &Path) -> PathBuf {
    super::super::path_utils::normalize_for_comparison(p)
}

// ---------------------------------------------------------------------------
// Git-dir / index helpers
// ---------------------------------------------------------------------------

/// Resolve the `.git` directory path for a repo (handles worktrees).
fn get_git_dir(repo_root: &Path) -> Result<PathBuf, GitAiError> {
    let args = vec![
        "-C".to_string(),
        repo_root.to_string_lossy().into_owned(),
        "rev-parse".to_string(),
        "--git-dir".to_string(),
    ];
    let output = crate::clients::git_cli::exec_git_allow_nonzero(&args)?;
    if !output.status.success() {
        return Err(GitAiError::Generic(
            "git rev-parse --git-dir failed".to_string(),
        ));
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if Path::new(&s).is_absolute() {
        Ok(PathBuf::from(s))
    } else {
        Ok(repo_root.join(s))
    }
}

/// Return the mtime of `.git/index` as nanoseconds since the UNIX epoch.
///
/// Used as a cold-start watermark proxy when the daemon has no worktree
/// watermark yet.  Only called when `wm = Some(w)` with `w.worktree = None`,
/// so passing `wm = None` (tests, non-daemon mode) always bypasses this.
pub fn git_index_mtime_ns(repo_root: &Path) -> Option<u128> {
    let git_dir = get_git_dir(repo_root).ok()?;
    let index_path = git_dir.join("index");
    let mtime = fs::metadata(&index_path).ok()?.modified().ok()?;
    Some(system_time_to_nanos(mtime))
}

// ---------------------------------------------------------------------------
// Watermark coverage
// ---------------------------------------------------------------------------

/// Test whether a file is covered by the current watermarks, meaning it has
/// not been modified since the last known-good baseline and does not need to
/// be stored in the snapshot.
///
/// A file is covered when:
/// - It has a per-file watermark AND `mtime ≤ file_wm + GRACE`, OR
/// - No per-file watermark but an effective worktree wm exists AND
///   `mtime ≤ effective_wm + GRACE`.
pub fn is_wm_covered(
    mtime_ns: u128,
    effective_wm: Option<u128>,
    per_file_wm: &HashMap<String, u128>,
    posix_key: &str,
) -> bool {
    if let Some(&file_wm) = per_file_wm.get(posix_key) {
        return mtime_ns <= file_wm + MTIME_GRACE_WINDOW_NS;
    }
    effective_wm.is_some_and(|ewm| mtime_ns <= ewm + MTIME_GRACE_WINDOW_NS)
}

// ---------------------------------------------------------------------------
// Gitignore filter
// ---------------------------------------------------------------------------

/// Build the git-ai ignore ruleset for use in `filter_entry` on the snapshot walker.
///
/// Only covers the git-ai-specific patterns:
/// - Default ignore patterns (lock files, node_modules, etc.)
/// - Patterns from `.git-ai-ignore` at the repo root
/// - Linguist-generated patterns from `.gitattributes` at the repo root
///
/// Standard `.gitignore` handling — including nested `.gitignore` files throughout
/// the repo tree — is left to `WalkBuilder` with `git_ignore(true)`, which discovers
/// and applies them natively as it descends. Adding them here too would be redundant
/// and would require a separate pre-walk that can't apply rules during traversal.
pub fn build_gitignore(repo_root: &Path) -> Result<Gitignore, GitAiError> {
    let mut builder = GitignoreBuilder::new(repo_root);

    // git-ai-specific patterns: same source of truth as non-bash checkpoints.
    let shared_patterns: Vec<String> = default_ignore_patterns()
        .into_iter()
        .chain(load_git_ai_ignore_patterns_from_path(repo_root))
        .chain(load_linguist_generated_patterns_from_path(repo_root))
        .collect();
    for pattern in &shared_patterns {
        if let Err(e) = builder.add_line(None, pattern) {
            tracing::debug!("Warning: failed to add ignore pattern '{}': {}", pattern, e);
        }
    }

    builder
        .build()
        .map_err(|e| GitAiError::Generic(format!("Failed to build gitignore rules: {}", e)))
}

/// Check whether a newly created (untracked) file should be included.
/// Returns true if the file is NOT ignored by .gitignore rules.
pub fn should_include_new_file(gitignore: &Gitignore, path: &Path, is_dir: bool) -> bool {
    // Use matched_path_or_any_parents so directory patterns like `secrets/` also
    // exclude files nested inside that directory (e.g. `secrets/token.txt`).
    let matched = gitignore.matched_path_or_any_parents(path, is_dir);
    !matched.is_ignore()
}
