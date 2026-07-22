//! Core data types for bash-tool stat-diff snapshots.

use crate::utils::normalize_to_posix;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

// ---------------------------------------------------------------------------
// StatEntry
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// StatSnapshot
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// StatDiffResult
// ---------------------------------------------------------------------------

/// Result of diffing two snapshots.
#[derive(Debug, Default)]
pub struct StatDiffResult {
    pub created: Vec<PathBuf>,
    pub modified: Vec<PathBuf>,
}

impl StatDiffResult {
    /// All changed paths (created + modified) as Strings.
    pub fn all_changed_paths(&self) -> Vec<String> {
        self.created
            .iter()
            .chain(self.modified.iter())
            .map(|p| normalize_to_posix(&p.to_string_lossy()))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.created.is_empty() && self.modified.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Hook result types
// ---------------------------------------------------------------------------

/// What the bash post-hook decided to do.
#[derive(Debug)]
pub enum BashCheckpointAction {
    /// Files changed — emit a checkpoint with these paths.
    Checkpoint(Vec<String>),
    /// Stat-diff ran but found nothing.
    NoChanges,
    /// The post-hook exceeded its time budget.
    HookTimeout,
    /// The post-snapshot filesystem walk failed (walk timeout, too many files, IO error).
    SnapshotFailed,
    /// The daemon had no pre-snapshot for this tool-use ID.
    MissingPreSnapshot,
}

/// Result from `handle_bash_pre_tool_use_with_context`.
pub struct BashPreHookResult {
    /// Files with mtime > watermark at pre-snapshot time (absolute paths).
    pub dirty_paths: Vec<PathBuf>,
}

/// Result from `handle_bash_post_tool_use`.
pub struct BashPostHookResult {
    /// The checkpoint action.
    pub action: BashCheckpointAction,
}

// ---------------------------------------------------------------------------
// ToolClass
// ---------------------------------------------------------------------------

/// Per-agent tool classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    /// A known file-edit tool (Write, Edit, etc.) — handled by existing preset logic.
    FileEdit,
    /// A bash/shell tool — handled by the stat-diff system.
    Bash,
    /// Unrecognized tool — skip checkpoint.
    Skip,
}
