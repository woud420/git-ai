//! Test-support-only working-log and ref-cursor trace side channel.
//!
//! This intentionally stays separate from checkpoint-outbox diagnostics. The
//! outbox sentinel is a redacted persisted health record; WLTRACE is an
//! opt-in, append-only timeline that can contain test paths and Git identities.

#[cfg(feature = "test-support")]
mod imp {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static TRACE_PATH: OnceLock<PathBuf> = OnceLock::new();

    /// Resolve `GIT_AI_WLTRACE` once, before command dispatch starts.
    pub fn initialize_from_env() {
        let Some(path) = std::env::var_os("GIT_AI_WLTRACE").map(PathBuf::from) else {
            return;
        };
        if path.as_os_str().is_empty() {
            return;
        }
        let _ = TRACE_PATH.set(path);
        ENABLED.store(TRACE_PATH.get().is_some(), Ordering::Relaxed);
    }

    /// Append one timeline record. When disabled, a call costs one relaxed
    /// atomic load and does not evaluate `detail`.
    #[inline]
    pub fn record(operation: &str, path: &Path, detail: impl FnOnce() -> String) {
        if !ENABLED.load(Ordering::Relaxed) {
            return;
        }
        record_enabled(operation, path, detail);
    }

    #[cold]
    #[inline(never)]
    fn record_enabled(operation: &str, path: &Path, detail: impl FnOnce() -> String) {
        let Some(trace_path) = TRACE_PATH.get() else {
            return;
        };
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let thread = std::thread::current();
        let detail = detail().replace('\r', "\\r").replace('\n', "\\n");
        let line = format!(
            "{timestamp} pid={} tid={:?} tname={:?} op={operation} path={path:?} {detail}\n",
            std::process::id(),
            thread.id(),
            thread.name().unwrap_or("-"),
        );
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(trace_path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    #[cfg(test)]
    pub(crate) fn enable_for_test(path: PathBuf) -> TestTraceGuard {
        let _ = TRACE_PATH.set(path);
        ENABLED.store(true, Ordering::Relaxed);
        TestTraceGuard
    }

    #[cfg(test)]
    pub(crate) struct TestTraceGuard;

    #[cfg(test)]
    impl Drop for TestTraceGuard {
        fn drop(&mut self) {
            ENABLED.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(not(feature = "test-support"))]
mod imp {
    use std::path::Path;

    #[inline(always)]
    pub fn initialize_from_env() {}

    #[inline(always)]
    pub fn record(_operation: &str, _path: &Path, _detail: impl FnOnce() -> String) {}
}

pub use imp::{initialize_from_env, record};

#[cfg(test)]
pub(crate) use imp::enable_for_test;
