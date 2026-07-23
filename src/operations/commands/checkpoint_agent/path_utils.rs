use std::path::{Path, PathBuf};

/// Normalize a path for lexical comparison on the current platform.
///
/// macOS and Windows commonly use case-insensitive filesystems, so paths
/// referring to the same location may differ only in letter casing.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn normalize_for_comparison(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn normalize_for_comparison(path: &Path) -> PathBuf {
    path.to_path_buf()
}
