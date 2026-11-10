//! Path normalization, git-dir helpers, watermark coverage checks, and
//! gitignore-based file filtering for the bash-tool stat-diff system.

use crate::error::GitAiError;
use crate::operations::authorship::ignore::{
    default_ignore_patterns, load_git_ai_ignore_patterns_from_path,
    load_linguist_generated_patterns_from_path,
};
use crate::operations::git::repo_state::git_dir_for_worktree;
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

/// Return the mtime of `.git/index` as nanoseconds since the UNIX epoch.
///
/// Used as a cold-start watermark proxy when the daemon has no worktree
/// watermark yet.  Only called when `wm = Some(w)` with `w.worktree = None`,
/// so passing `wm = None` (tests, non-daemon mode) always bypasses this.
pub fn git_index_mtime_ns(repo_root: &Path) -> Option<u128> {
    let git_dir = git_dir_for_worktree(repo_root)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::git::test_utils::TmpRepo;
    use std::time::{Duration, SystemTime};

    /// The index must be the one git itself uses, including where `.git` is a
    /// `gitdir:` pointer file (linked worktree, submodule).
    #[test]
    fn git_index_mtime_ns_reads_the_index_git_uses() {
        let repo = TmpRepo::new().unwrap();
        repo.write_file("f", "x", true).unwrap();
        repo.commit_all("init").unwrap();
        let library = TmpRepo::new().unwrap();
        library.commit_all("init").unwrap();

        let linked = repo.path().join("linked");
        let submodule = repo.path().join("sub");
        repo.git_command(&["worktree", "add", linked.to_str().unwrap()])
            .unwrap();
        let library_url = library.path().to_str().unwrap();
        repo.git_command(&[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            library_url,
            "sub",
        ])
        .unwrap();

        for (n, root) in [repo.path(), &linked, &submodule].into_iter().enumerate() {
            let index = repo
                .git_command(&[
                    "-C",
                    root.to_str().unwrap(),
                    "rev-parse",
                    "--git-path",
                    "index",
                ])
                .unwrap();
            // Distinct, known mtimes so reading a sibling's index cannot pass.
            let stamp = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 * (n as u64 + 1));
            fs::File::options()
                .write(true)
                .open(root.join(index.trim()))
                .unwrap()
                .set_modified(stamp)
                .unwrap();

            assert_eq!(
                git_index_mtime_ns(root),
                Some(system_time_to_nanos(stamp)),
                "{}",
                root.display()
            );
        }
    }
}
