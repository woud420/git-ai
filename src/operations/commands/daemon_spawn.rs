use super::daemon_runtime_dir;
use crate::operations::daemon::DaemonConfig;
use std::process::{Command, Stdio};

#[cfg(windows)]
#[path = "daemon_spawn_windows.rs"]
mod windows;

pub(super) fn spawn_daemon_run_detached(config: &DaemonConfig) -> Result<(), String> {
    // Resolve through a git shim: spawning that symlink would re-enter Git
    // dispatch instead of starting the daemon.
    let exe = crate::cli::git_ai_exe::current_git_ai_exe().map_err(|error| error.to_string())?;
    let mut child = Command::new(exe);
    child
        .args(["bg", "run"])
        .current_dir(daemon_runtime_dir(config)?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::operations::daemon::sanitize_daemon_child_environment(&mut child);
    child.env_remove("GIT_AI");

    #[cfg(windows)]
    {
        windows::spawn(&mut child)
    }
    #[cfg(not(windows))]
    {
        child.spawn().map(|_| ()).map_err(|error| error.to_string())
    }
}
