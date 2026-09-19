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

    /// Retry exclusive access for a bounded interval. Windows readers can briefly
    /// prevent the exclusive open even when no other daemon owns the lock.
    pub fn acquire_with_timeout(
        path: &std::path::Path,
        timeout: std::time::Duration,
    ) -> Option<Self> {
        let started = std::time::Instant::now();
        loop {
            if let Some(lock) = Self::try_acquire(path) {
                return Some(lock);
            }
            let remaining = timeout.checked_sub(started.elapsed())?;
            if remaining.is_zero() {
                return None;
            }
            std::thread::sleep(remaining.min(std::time::Duration::from_millis(25)));
        }
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
mod tests;
