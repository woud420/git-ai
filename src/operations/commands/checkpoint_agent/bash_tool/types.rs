//! Core data types for bash-tool stat-diff snapshots.

use crate::operations::git::path_format::normalize_to_posix;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// StatEntry / StatFileType / StatSnapshot
// ---------------------------------------------------------------------------

// These are wire-shape DTOs (serialized over the daemon control socket) that
// live in `model` alongside the rest of the daemon control DTOs. Re-exported
// here so existing callers that import `StatEntry`/`StatFileType`/
// `StatSnapshot` from this module keep working.
pub use crate::model::stat_snapshot::StatEntry;
pub use crate::model::stat_snapshot::StatFileType;
pub use crate::model::stat_snapshot::StatSnapshot;

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
