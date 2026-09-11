use crate::model::attribution::Attribution;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ResolvedCheckpointExecution {
    pub base_commit: String,
    pub ts: u128,
    pub files: Vec<String>,
    pub dirty_files: HashMap<String, Arc<str>>,
}

/// Latest checkpoint state needed to process a file in the next checkpoint.
#[derive(Debug, Clone)]
pub(super) struct PreviousFileState {
    pub(super) blob_sha: String,
    pub(super) attributions: Vec<Attribution>,
}

/// Per-file line statistics (in-memory only, not persisted)
#[derive(Debug, Clone, Default)]
#[doc(hidden)]
pub struct FileLineStats {
    pub additions: u32,
    pub deletions: u32,
    pub additions_sloc: u32,
    pub deletions_sloc: u32,
}
