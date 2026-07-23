//! Shared path-canonicalization helper.

use std::path::{Path, PathBuf};

/// Canonicalize `path`, falling back to an owned copy of `path` itself if
/// canonicalization fails (e.g. the path doesn't exist yet, or a permission
/// error). This is the "best effort" canonicalization idiom used throughout
/// the daemon and git layers for symlink-resolved path comparisons where a
/// failure to canonicalize should never be fatal.
pub fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
