//! Filesystem stat-snapshot DTOs used by bash-tool change detection.
//!
//! These are pure data shapes serialized over the daemon control socket.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

/// Metadata fingerprint for a single file, collected via `lstat()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatEntry {
    pub exists: bool,
    pub mtime: Option<SystemTime>,
    pub ctime: Option<SystemTime>,
    pub size: u64,
    pub mode: u32,
    pub file_type: StatFileType,
}

/// File type enumeration (symlink-aware, no following).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatFileType {
    Regular,
    Directory,
    Symlink,
    Other,
}

impl StatEntry {
    /// Build a `StatEntry` from `std::fs::Metadata` (from `symlink_metadata` / `lstat`).
    pub fn from_metadata(meta: &fs::Metadata) -> Self {
        let file_type = if meta.file_type().is_symlink() {
            StatFileType::Symlink
        } else if meta.file_type().is_dir() {
            StatFileType::Directory
        } else if meta.file_type().is_file() {
            StatFileType::Regular
        } else {
            StatFileType::Other
        };

        let mtime = meta.modified().ok();
        let size = meta.len();
        let mode = Self::extract_mode(meta);
        let ctime = Self::extract_ctime(meta);

        StatEntry {
            exists: true,
            mtime,
            ctime,
            size,
            mode,
            file_type,
        }
    }

    #[cfg(unix)]
    fn extract_mode(meta: &fs::Metadata) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode()
    }

    #[cfg(not(unix))]
    fn extract_mode(meta: &fs::Metadata) -> u32 {
        if meta.permissions().readonly() {
            0o444
        } else {
            0o644
        }
    }

    #[cfg(unix)]
    fn extract_ctime(meta: &fs::Metadata) -> Option<SystemTime> {
        use std::os::unix::fs::MetadataExt;
        let ctime_secs = meta.ctime();
        let ctime_nsecs = meta.ctime_nsec() as u32;
        if ctime_secs >= 0 {
            Some(SystemTime::UNIX_EPOCH + std::time::Duration::new(ctime_secs as u64, ctime_nsecs))
        } else {
            None
        }
    }

    #[cfg(not(unix))]
    fn extract_ctime(meta: &fs::Metadata) -> Option<SystemTime> {
        // On Windows, use creation time as a proxy for ctime
        meta.created().ok()
    }
}

/// A complete filesystem snapshot: stat-tuples keyed by normalized path.
///
/// Only stores entries for files that pass the git-ai ignore filter AND have
/// `mtime > effective_worktree_wm + GRACE` (i.e., not covered by any watermark).
/// Filtering is applied uniformly to all files — there is no special treatment
/// for git-tracked vs untracked files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatSnapshot {
    /// File metadata for files that passed the ignore filter and are not
    /// covered by any watermark at snapshot time.
    pub entries: HashMap<PathBuf, StatEntry>,
    /// When this snapshot was taken.
    #[serde(skip)]
    pub taken_at: Option<Instant>,
    /// Unique invocation key: "{session_id}:{tool_use_id}".
    pub invocation_key: String,
    /// Repo root path.
    pub repo_root: PathBuf,
    /// Effective worktree-level watermark at snapshot time.
    /// Either the real daemon worktree watermark (warm start) or the mtime
    /// of `.git/index` (cold-start proxy).  `None` if neither was available.
    #[serde(default)]
    pub effective_worktree_wm: Option<u128>,
    /// Per-file watermarks from the daemon at snapshot time.
    /// Used for Tier-1 stale detection in `find_stale_files`.
    #[serde(default)]
    pub per_file_wm: HashMap<String, u128>,
}
