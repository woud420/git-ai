//! Cross-platform exclusive file locking, used to coordinate single-instance
//! access to persistent daemon state (e.g. the daemon lock file).

/// A cross-platform exclusive file lock.
///
/// Holds an exclusive advisory lock (Unix) or exclusive-access file handle (Windows)
/// for the lifetime of the struct. The lock is automatically released when dropped
/// or when the process exits.
pub struct LockFile {
    _file: std::fs::File,
}

impl LockFile {
    /// Try to acquire an exclusive lock on the given path.
    /// Returns `Some(LockFile)` if successful, `None` if another process holds the lock.
    pub fn try_acquire(path: &std::path::Path) -> Option<Self> {
        let file = try_lock_exclusive(path)?;
        Some(Self { _file: file })
    }
}

#[cfg(unix)]
impl Drop for LockFile {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        unsafe { libc::flock(self._file.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(unix)]
#[allow(clippy::suspicious_open_options)]
fn try_lock_exclusive(path: &std::path::Path) -> Option<std::fs::File> {
    use std::os::unix::io::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .ok()?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return None;
    }
    Some(file)
}

#[cfg(windows)]
#[allow(clippy::suspicious_open_options)]
fn try_lock_exclusive(path: &std::path::Path) -> Option<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lockfile_acquire_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("test.lock");
        let lock = LockFile::try_acquire(&lock_path);
        assert!(lock.is_some(), "should acquire lock on a fresh path");
    }

    #[test]
    fn test_lockfile_second_acquire_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("test.lock");
        let _first = LockFile::try_acquire(&lock_path).expect("first acquire should succeed");
        let second = LockFile::try_acquire(&lock_path);
        assert!(second.is_none(), "second acquire should be blocked");
    }

    #[test]
    fn test_lockfile_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("test.lock");
        {
            let _lock = LockFile::try_acquire(&lock_path).expect("first acquire should succeed");
            // _lock is dropped here
        }
        let second = LockFile::try_acquire(&lock_path);
        assert!(
            second.is_some(),
            "should acquire lock after previous holder is dropped"
        );
    }

    #[test]
    fn test_lockfile_nonexistent_parent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("no_such_dir").join("test.lock");
        let lock = LockFile::try_acquire(&lock_path);
        assert!(
            lock.is_none(),
            "should return None when parent directory does not exist"
        );
    }
}
