#[cfg(unix)]
use super::fs;
use super::{Command, DaemonConfig, Path, PathBuf, TestRepo};

pub(super) fn daemon_control_socket_path(repo: &TestRepo) -> PathBuf {
    repo.daemon_control_socket_path()
}

pub(super) fn daemon_trace_socket_path(repo: &TestRepo) -> PathBuf {
    repo.daemon_trace_socket_path()
}

pub(super) fn daemon_lock_path(repo: &TestRepo) -> PathBuf {
    DaemonConfig::from_home(&repo.daemon_home_path()).lock_path
}

#[cfg(unix)]
pub(super) struct ColdDaemonSocketPaths {
    pub(super) directory: PathBuf,
    pub(super) control: PathBuf,
    pub(super) trace: PathBuf,
}

#[cfg(unix)]
impl ColdDaemonSocketPaths {
    pub(super) fn new(repo: &TestRepo) -> Self {
        let test_key = repo
            .test_home_path()
            .file_name()
            .expect("test home should have a final path component");
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("bs")
            .join(test_key);
        fs::create_dir_all(&directory).expect("cold-daemon socket directory should be creatable");
        let control = directory.join("c");
        let trace = directory.join("t");
        assert!(
            control.as_os_str().as_encoded_bytes().len() < 100
                && trace.as_os_str().as_encoded_bytes().len() < 100,
            "cold-daemon test socket paths must stay below Unix socket limits"
        );
        Self {
            directory,
            control,
            trace,
        }
    }
}

#[cfg(unix)]
impl Drop for ColdDaemonSocketPaths {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub(super) fn get_rss_kb(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb_str = rest.trim().trim_end_matches(" kB").trim();
            return kb_str.parse().ok();
        }
    }
    None
}

pub(super) fn repo_workdir_string(repo: &TestRepo) -> String {
    repo.path().to_string_lossy().to_string()
}

pub(super) fn configure_test_home_env(command: &mut Command, test_home: &Path) {
    command.env("HOME", test_home);
    command.env("GIT_CONFIG_GLOBAL", test_home.join(".gitconfig"));
    // Redirect XDG_CONFIG_HOME so git does not read the real user's
    // $XDG_CONFIG_HOME/git/config (which may contain filter drivers,
    // aliases, or other settings that break test isolation).
    command.env("XDG_CONFIG_HOME", test_home.join(".config"));
    // Suppress system-level git config (e.g., Xcode credential helpers)
    // that could interfere with test isolation.
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    // Sanitize PATH to remove directories containing the Nix git-ai
    // wrapper.  When the wrapper (a release build) runs with HOME
    // pointing to the test home it starts a background daemon at
    // the test socket path, poisoning the test environment.
    if let Ok(path) = std::env::var("PATH") {
        let sanitized: Vec<&str> = path
            .split(':')
            .filter(|dir| {
                // Keep only dirs that do NOT contain a git-ai wrapper
                // (heuristic: skip dirs where the `git` binary is a
                //  shell-script wrapper for git-ai, or a symlink to git-ai).
                let git_path = std::path::Path::new(dir).join("git");
                if git_path.is_file() || git_path.is_symlink() {
                    if let Ok(contents) = std::fs::read_to_string(&git_path)
                        && contents.contains("git-ai")
                    {
                        return false;
                    }
                    if let Ok(target) = std::fs::read_link(&git_path)
                        && target.to_string_lossy().contains("git-ai")
                    {
                        return false;
                    }
                    if let Ok(canonical) = git_path.canonicalize()
                        && canonical.to_string_lossy().contains("git-ai")
                    {
                        return false;
                    }
                }
                true
            })
            .collect();
        command.env("PATH", sanitized.join(":"));
    }
    #[cfg(windows)]
    {
        command.env("USERPROFILE", test_home);
        command.env("APPDATA", test_home.join("AppData").join("Roaming"));
        command.env("LOCALAPPDATA", test_home.join("AppData").join("Local"));
    }
}

pub(super) fn configure_test_daemon_env(
    command: &mut Command,
    daemon_home: &Path,
    control_socket_path: &Path,
    trace_socket_path: &Path,
) {
    command.env("GIT_AI_DAEMON_HOME", daemon_home);
    command.env("GIT_AI_DAEMON_CONTROL_SOCKET", control_socket_path);
    command.env("GIT_AI_DAEMON_TRACE_SOCKET", trace_socket_path);
}

pub(super) fn unique_worktree_path(repo: &TestRepo, prefix: &str) -> PathBuf {
    repo.path().parent().unwrap_or(repo.path()).join(format!(
        "{}-{}",
        prefix,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}
