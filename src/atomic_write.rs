use crate::error::GitAiError;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Text-dedup constructor for the atomic-write filesystem errors below;
/// Display output is unchanged from the sites it replaces.
fn atomic_write_io_error(
    what: &str,
    location: impl std::fmt::Display,
    e: std::io::Error,
) -> GitAiError {
    GitAiError::Generic(format!("Failed to {} {}: {}", what, location, e))
}

/// Write data to a file atomically (write to temp, then rename)
/// If the path is a symlink, writes to the target file (preserving the symlink)
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), GitAiError> {
    write_atomic_with_privacy(path, data, false)
}

/// [`write_atomic`] for secrets: the file is owner-only (0600 on Unix) from
/// creation, whatever the mode of the file it replaces.
pub fn write_private_atomic(path: &Path, data: &[u8]) -> Result<(), GitAiError> {
    write_atomic_with_privacy(path, data, true)
}

fn write_atomic_with_privacy(path: &Path, data: &[u8], owner_only: bool) -> Result<(), GitAiError> {
    let target_path = if path.is_symlink() {
        fs::canonicalize(path)
            .map_err(|e| atomic_write_io_error("resolve symlink", path.display(), e))?
    } else {
        path.to_path_buf()
    };

    // Ensure parent directory exists before writing. This guards against
    // environments (e.g. nushell) where the parent may not yet exist when
    // write_atomic is reached.
    ensure_parent_dir(&target_path)?;

    // The rename swaps in a new inode: keep the mode of the file being
    // replaced so a private (0600) config stays private.
    #[cfg(unix)]
    let mode = if owner_only {
        Some(0o600)
    } else {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(&target_path)
            .ok()
            .map(|existing| existing.permissions().mode())
    };
    #[cfg(not(unix))]
    let _ = owner_only;

    let tmp_path = target_path.with_extension("tmp");
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if let Some(mode) = mode {
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, mode);
        }
        let mut file = options
            .open(&tmp_path)
            .map_err(|e| atomic_write_io_error("create temp file", tmp_path.display(), e))?;
        // Also covers the umask and a stale temp file left with another mode.
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        file.write_all(data)?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, &target_path).map_err(|e| {
        atomic_write_io_error(
            "rename",
            format!("{} to {}", tmp_path.display(), target_path.display()),
            e,
        )
    })?;
    Ok(())
}

/// Ensure parent directory exists
fn ensure_parent_dir(path: &Path) -> Result<(), GitAiError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| atomic_write_io_error("create directory", parent.display(), e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_write_atomic_regular_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        write_atomic(&file_path, b"hello world").unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello world");
        assert!(!file_path.is_symlink());
    }

    #[test]
    #[cfg(unix)]
    fn test_write_atomic_preserves_symlink() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().unwrap();

        // Create the actual target file in a subdirectory (simulating dotfiles)
        let target_dir = temp_dir.path().join("dotfiles");
        fs::create_dir_all(&target_dir).unwrap();
        let target_file = target_dir.join("settings.json");
        fs::write(&target_file, r#"{"original": true}"#).unwrap();

        // Create a symlink pointing to the target file
        let symlink_path = temp_dir.path().join("settings.json");
        symlink(&target_file, &symlink_path).unwrap();

        // Verify symlink is set up correctly
        assert!(symlink_path.is_symlink());
        assert_eq!(fs::read_link(&symlink_path).unwrap(), target_file);

        // Write through the symlink using write_atomic
        write_atomic(&symlink_path, b"updated content").unwrap();

        // The symlink should still exist and point to the same target
        assert!(symlink_path.is_symlink(), "symlink should be preserved");
        assert_eq!(
            fs::read_link(&symlink_path).unwrap(),
            target_file,
            "symlink target should be unchanged"
        );

        // The target file should have the new content
        let target_content = fs::read_to_string(&target_file).unwrap();
        assert_eq!(target_content, "updated content");

        // Reading through the symlink should also return the new content
        let symlink_content = fs::read_to_string(&symlink_path).unwrap();
        assert_eq!(symlink_content, "updated content");
    }

    #[test]
    #[cfg(unix)]
    fn test_write_atomic_preserves_relative_symlink() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().unwrap();

        // Create the actual target file in a subdirectory
        let target_dir = temp_dir.path().join("dotfiles").join("config");
        fs::create_dir_all(&target_dir).unwrap();
        let target_file = target_dir.join("settings.json");
        fs::write(&target_file, r#"{"original": true}"#).unwrap();

        // Create a directory for the symlink
        let link_dir = temp_dir.path().join(".config");
        fs::create_dir_all(&link_dir).unwrap();

        // Create a relative symlink
        let symlink_path = link_dir.join("settings.json");
        let relative_target = PathBuf::from("../dotfiles/config/settings.json");
        symlink(&relative_target, &symlink_path).unwrap();

        // Verify symlink is set up correctly
        assert!(symlink_path.is_symlink());

        // Write through the symlink using write_atomic
        write_atomic(&symlink_path, b"relative symlink content").unwrap();

        // The symlink should still exist
        assert!(symlink_path.is_symlink(), "symlink should be preserved");

        // The target file should have the new content
        let target_content = fs::read_to_string(&target_file).unwrap();
        assert_eq!(target_content, "relative symlink content");
    }

    /// The rename replaces the target's inode, so the mode has to be carried
    /// over or a private config (API keys) becomes world-readable.
    #[test]
    #[cfg(unix)]
    fn test_write_atomic_preserves_existing_file_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("config.toml");
        fs::write(&file_path, "api_key = \"secret\"").unwrap();
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o600)).unwrap();

        write_atomic(&file_path, b"updated").unwrap();

        assert_eq!(fs::read_to_string(&file_path).unwrap(), "updated");
        let mode = fs::metadata(&file_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// write_atomic should create parent directories
    /// if they do not exist, preventing "No such file or directory" errors.
    #[test]
    fn test_write_atomic_creates_parent_dirs() {
        let temp_dir = TempDir::new().unwrap();
        // Path whose parent directory does NOT yet exist
        let file_path = temp_dir
            .path()
            .join("nonexistent")
            .join("subdir")
            .join("test.json");
        assert!(!file_path.parent().unwrap().exists());

        write_atomic(&file_path, b"{\"key\": \"value\"}").unwrap();

        assert!(file_path.exists());
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "{\"key\": \"value\"}");
    }

    /// ensure_parent_dir handles nested missing directories.
    #[test]
    fn test_ensure_parent_dir_creates_nested() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("file.txt");
        assert!(!temp_dir.path().join("a").exists());

        ensure_parent_dir(&file_path).unwrap();

        assert!(file_path.parent().unwrap().exists());
    }

    /// ensure_parent_dir is a no-op for root-level paths.
    #[test]
    fn test_ensure_parent_dir_no_parent() {
        // A path with no parent component should not error
        let path = Path::new("standalone_file.txt");
        ensure_parent_dir(path).unwrap();
    }
}
