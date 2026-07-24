//! Filesystem snapshot, diff, and git-status fallback for bash-tool attribution.

use crate::error::GitAiError;
use crate::operations::git::path_format::normalize_to_posix;
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use super::daemon_api::DaemonWatermarks;
use super::path_filter::{build_gitignore, is_wm_covered, normalize_path, should_include_new_file};
use super::types::{StatEntry, StatSnapshot};
use super::{MAX_TRACKED_FILES, effective_walk_timeout_ms};

// ---------------------------------------------------------------------------
// system_time_to_nanos
// ---------------------------------------------------------------------------

/// Convert a `SystemTime` to nanoseconds since UNIX epoch for watermark comparison.
pub(super) fn system_time_to_nanos(t: SystemTime) -> u128 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

/// Take a stat snapshot of the repo working tree.
///
/// Only stores entries for files that pass the git-ai ignore filter (gitignore
/// + defaults + .git-ai-ignore + linguist) AND have `mtime > effective_wm + GRACE`.
///
/// Filtering is applied uniformly to all files — there is no special treatment
/// for git-tracked vs untracked files.
///
/// `wm` should be the result of a recent daemon watermark query.  Pass
/// `None` to skip watermark filtering entirely (no daemon context, or direct
/// `snapshot()` callers such as tests and `git_status_fallback`).
pub fn snapshot(
    repo_root: &Path,
    session_id: &str,
    tool_use_id: &str,
    wm: Option<&DaemonWatermarks>,
) -> Result<StatSnapshot, GitAiError> {
    let start = Instant::now();
    let invocation_key = format!("{}:{}", session_id, tool_use_id);

    // Compute the effective worktree-level watermark:
    //   wm = Some(w) with real worktree wm → use it directly (warm start).
    //   wm = Some(w) with no worktree wm → daemon up but hasn't seen a full
    //                Human checkpoint yet; use .git/index mtime as proxy.
    //   wm = None   → no filtering (caller opted out or direct snapshot() call
    //                without daemon context).
    //
    // Note: the cold-start proxy (git_index_mtime_ns) is injected by
    // handle_bash_pre_tool_use_with_context when no daemon is running, not here, so direct
    // snapshot() callers (e.g. tests, git_status_fallback) are unaffected.
    let effective_worktree_wm: Option<u128> = match wm {
        Some(w) if w.worktree.is_some() => w.worktree,
        Some(_) => super::path_filter::git_index_mtime_ns(repo_root),
        None => None,
    };

    let per_file_wm: HashMap<String, u128> = wm.map(|w| w.per_file.clone()).unwrap_or_default();

    // Build the git-ai ignore ruleset: gitignore + defaults + .git-ai-ignore + linguist.
    // Arc is needed because filter_entry requires 'static, preventing a borrow.
    // The closure takes sole ownership; no post-walker use of the ruleset is needed.
    let gitignore_filter = Arc::new(build_gitignore(repo_root)?);

    let mut entries = HashMap::new();

    // Pass the git-ai ignore ruleset directly into the walker via filter_entry.
    // This prunes entire ignored directories (node_modules/, target/, etc.)
    // before the walker descends into them — including directories that are in
    // default_ignore_patterns() but not yet in the repo's .gitignore (a common
    // case for node_modules that the user hasn't gitignored yet).
    // git_ignore(true) handles the standard .gitignore case; filter_entry
    // catches the rest (defaults, .git-ai-ignore, linguist-generated).
    let repo_root_buf = repo_root.to_path_buf();
    let walker = WalkBuilder::new(repo_root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(move |entry| {
            if entry.file_name() == ".git" {
                return false;
            }
            let abs = entry.path();
            let Ok(rel) = abs.strip_prefix(&repo_root_buf) else {
                return true; // outside repo root — let walker handle it
            };
            if rel.as_os_str().is_empty() {
                return true; // repo root itself — always include
            }
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            should_include_new_file(&gitignore_filter, rel, is_dir)
        })
        .build();

    let walk_timeout = Duration::from_millis(effective_walk_timeout_ms());
    for result in walker {
        let elapsed = start.elapsed();
        if elapsed >= walk_timeout {
            let elapsed_ms = elapsed.as_millis();
            let timeout_ms = walk_timeout.as_millis();
            let msg = format!(
                "bash_tool: snapshot walk exceeded {}ms limit ({}ms elapsed, {} entries so far); abandoning stat-diff",
                timeout_ms,
                elapsed_ms,
                entries.len()
            );
            tracing::debug!("{}", msg);
            crate::observability::log_message(
                &msg,
                "warning",
                Some(serde_json::json!({
                    "elapsed_ms": elapsed_ms,
                    "entries_so_far": entries.len(),
                    "walk_timeout_ms": timeout_ms,
                })),
            );
            return Err(GitAiError::Generic(msg));
        }

        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("Walker error: {}", e);
                continue;
            }
        };

        let abs_path = entry.path();

        // Skip directories — filter_entry already pruned ignored dirs; this
        // guard drops any remaining directory entries (e.g. the repo root).
        if entry
            .file_type()
            .map(|ft| ft.is_dir())
            .unwrap_or_else(|| abs_path.is_dir())
        {
            continue;
        }

        let rel_path = match abs_path.strip_prefix(repo_root) {
            Ok(p) => p,
            Err(_) => continue,
        };

        // filter_entry already applied should_include_new_file for files too,
        // so no secondary check is needed here.

        let normalized = normalize_path(rel_path);

        match fs::symlink_metadata(abs_path) {
            Ok(meta) => {
                let stat = StatEntry::from_metadata(&meta);
                let mtime_ns = stat.mtime.map(system_time_to_nanos).unwrap_or(0);
                let posix_key = normalize_to_posix(&normalized.to_string_lossy());
                if !is_wm_covered(mtime_ns, effective_worktree_wm, &per_file_wm, &posix_key) {
                    entries.insert(normalized, stat);
                    if entries.len() > MAX_TRACKED_FILES {
                        tracing::debug!(
                            "Snapshot: exceeded MAX_TRACKED_FILES ({}), skipping stat-diff",
                            MAX_TRACKED_FILES
                        );
                        return Err(GitAiError::Generic(format!(
                            "repo has more than {} recently-modified files; skipping stat-diff",
                            MAX_TRACKED_FILES
                        )));
                    }
                }
            }
            Err(e) => {
                tracing::debug!("Failed to stat {}: {}", abs_path.display(), e);
            }
        }
    }

    tracing::debug!(
        "Snapshot: {} files scanned in {}ms",
        entries.len(),
        start.elapsed().as_millis()
    );

    Ok(StatSnapshot {
        entries,
        taken_at: Some(Instant::now()),
        invocation_key,
        repo_root: repo_root.to_path_buf(),
        effective_worktree_wm,
        per_file_wm,
    })
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

/// Diff two snapshots to find created and modified files.
///
/// Both snapshots apply the same git-ai ignore filter at snapshot time, so
/// any file in `post.entries` already passed that filter. No secondary
/// filtering is needed here.
///
/// Files in post but not pre are reported as **created** (either genuinely
/// new, or previously wm-covered and now modified by bash — both are changed
/// files that need attribution).  Files in both with a changed stat-tuple are
/// reported as **modified**.  Deletions are not tracked.
pub fn diff(pre: &StatSnapshot, post: &StatSnapshot) -> super::types::StatDiffResult {
    let mut result = super::types::StatDiffResult::default();

    // Files in post but not pre: new files or previously wm-covered files
    // now modified by bash. Both need attribution; the distinction doesn't
    // matter since all_changed_paths() merges created + modified.
    for path in post.entries.keys() {
        if !pre.entries.contains_key(path) {
            result.created.push(path.clone());
        }
    }

    // Files in both but stat-tuple differs.
    for (path, post_entry) in &post.entries {
        if let Some(pre_entry) = pre.entries.get(path)
            && pre_entry != post_entry
        {
            result.modified.push(path.clone());
        }
    }

    result.created.sort();
    result.modified.sort();

    result
}

// ---------------------------------------------------------------------------
// Git status fallback
// ---------------------------------------------------------------------------

/// Build the args for a `git status --porcelain=v2` fallback.
pub fn git_status_fallback_args(repo_root: &Path) -> Vec<String> {
    vec![
        "-C".to_string(),
        repo_root.to_string_lossy().into_owned(),
        "--no-optional-locks".to_string(),
        "status".to_string(),
        "--porcelain=v2".to_string(),
        "-z".to_string(),
        "--untracked-files=all".to_string(),
    ]
}

/// Fall back to `git status --porcelain=v2` to detect changed files.
/// Used when the pre-snapshot is lost (process restart) or on very large repos.
pub fn git_status_fallback(repo_root: &Path) -> Result<Vec<String>, GitAiError> {
    let args = git_status_fallback_args(repo_root);
    let output = crate::clients::git_cli::exec_git_allow_nonzero(&args)?;

    if !output.status.success() {
        return Err(GitAiError::Generic(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let mut changed_files = Vec::new();
    let parts: Vec<&[u8]> = output.stdout.split(|&b| b == 0).collect();
    let mut i = 0;
    while i < parts.len() {
        let part = parts[i];
        if part.is_empty() {
            i += 1;
            continue;
        }

        let line = String::from_utf8_lossy(part);

        if line.starts_with("1 ") || line.starts_with("u ") {
            // Ordinary entry: 8 fields before path; unmerged: 10 fields before path
            let n = if line.starts_with("u ") { 11 } else { 9 };
            let fields: Vec<&str> = line.splitn(n, ' ').collect();
            if let Some(path) = fields.last() {
                changed_files.push(normalize_to_posix(path));
            }
        } else if line.starts_with("2 ") {
            // Rename/copy: 9 fields before new path, then NUL-delimited original path
            let fields: Vec<&str> = line.splitn(10, ' ').collect();
            if let Some(path) = fields.last() {
                changed_files.push(normalize_to_posix(path));
            }
            // Also include the original path (next NUL-delimited entry)
            if i + 1 < parts.len() {
                let orig = String::from_utf8_lossy(parts[i + 1]);
                if !orig.is_empty() {
                    changed_files.push(normalize_to_posix(&orig));
                }
            }
            i += 1;
        } else if let Some(path) = line.strip_prefix("? ") {
            // Untracked: path follows "? "
            changed_files.push(normalize_to_posix(path));
        }

        i += 1;
    }

    Ok(changed_files)
}
