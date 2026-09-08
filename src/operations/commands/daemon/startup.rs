use super::arguments::has_flag;
use crate::model::repository::lock_file::LockFile;
use crate::operations::commands::daemon_start_policy::{
    SandboxMarkers, require_detached_start_allowed,
};
use crate::operations::daemon::{
    ControlRequest, DaemonConfig, local_socket_connects_with_timeout, remove_stale_daemon_files,
    send_control_request_with_timeout,
};
#[cfg(windows)]
use crate::process_spawn::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};
#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(super) fn handle_start(args: &[String]) -> Result<(), String> {
    if has_flag(args, "--mode") {
        return Err("--mode is no longer supported; daemon always runs in write mode".to_string());
    }
    ensure_daemon_running_attached(daemon_startup_timeout()).map(|_| ())
}

pub(super) fn daemon_startup_timeout() -> Duration {
    #[cfg(windows)]
    {
        if std::env::var_os("GIT_AI_TEST_DB_PATH").is_some()
            || std::env::var_os("GITAI_TEST_DB_PATH").is_some()
            || std::env::var_os("CI").is_some()
        {
            return Duration::from_secs(12);
        }

        Duration::from_secs(5)
    }

    #[cfg(not(windows))]
    {
        Duration::from_secs(2)
    }
}

/// Spawn a daemon and wait for it to become healthy. Used by explicit CLI
/// commands (`bg start`, `bg restart`) — NOT guarded for test builds.
///
/// On Unix, spawns with piped stderr so startup failures are surfaced to the
/// user. On Windows, spawns fully detached (null stdio) because piped handles
/// cause the parent to hang when the daemon outlives it.
pub(super) fn ensure_daemon_running_attached(timeout: Duration) -> Result<DaemonConfig, String> {
    let config = daemon_config_from_env_or_default_paths()?;
    if daemon_is_up(&config) {
        return Ok(config);
    }

    let markers = SandboxMarkers::from_env();
    require_detached_start_allowed(&markers)?;

    remove_stale_daemon_files(&config);

    if daemon_startup_is_blocked(&config) {
        return Err(format!(
            "daemon startup blocked: lock held at {}",
            config.lock_path.display()
        ));
    }

    #[cfg(not(windows))]
    {
        let mut child = spawn_daemon_run_with_piped_stderr(&config)?;
        let deadline = Instant::now() + timeout;
        loop {
            if daemon_is_up(&config) {
                return Ok(config);
            }
            match child.try_wait() {
                Ok(Some(status)) if !status.success() => {
                    let mut stderr_buf = String::new();
                    if let Some(mut stderr) = child.stderr.take() {
                        use std::io::Read;
                        let _ = stderr.read_to_string(&mut stderr_buf);
                    }
                    let detail = if stderr_buf.trim().is_empty() {
                        format!("daemon process exited with {}", status)
                    } else {
                        stderr_buf.trim().to_string()
                    };
                    return Err(format!("daemon failed to start: {}", detail));
                }
                Ok(Some(_)) => {
                    return Err("daemon process exited before sockets were ready".to_string());
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "timed out after {:?} waiting for daemon sockets {} and {}",
                    timeout,
                    config.control_socket_path.display(),
                    config.trace_socket_path.display()
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(windows)]
    {
        spawn_daemon_run_detached(&config)?;
        if wait_for_daemon_up(&config, timeout) {
            return Ok(config);
        }
        Err(format!(
            "timed out after {:?} waiting for daemon sockets {} and {}",
            timeout,
            config.control_socket_path.display(),
            config.trace_socket_path.display()
        ))
    }
}

pub(super) fn daemon_config_from_env_or_default_paths() -> Result<DaemonConfig, String> {
    DaemonConfig::from_env_or_default_paths().map_err(|e| e.to_string())
}

pub(crate) fn ensure_daemon_running(
    #[cfg_attr(any(test, feature = "test-support"), allow(unused))] timeout: Duration,
) -> Result<DaemonConfig, String> {
    let config = daemon_config_from_env_or_default_paths()?;
    if daemon_is_up(&config) {
        return Ok(config);
    }

    // Test auto-spawn is opt-in for one serial regression; otherwise the
    // harness retains its guard against parallel process storms.
    #[cfg(any(test, feature = "test-support"))]
    {
        match std::env::var("GIT_AI_TEST_ALLOW_DAEMON_AUTOSPAWN") {
            Ok(value) if value == "1" => ensure_daemon_running_attached(timeout),
            _ => Err("daemon not running (test build: auto-spawn disabled)".to_string()),
        }
    }

    #[cfg(not(any(test, feature = "test-support")))]
    {
        use crate::operations::commands::daemon_start_policy::should_auto_start_detached;
        let auto_start_disabled = std::env::var("_GITAI_INTERNAL_DISABLE_WRAPPER_DAEMON_AUTOSPAWN")
            .is_ok_and(|v| v == "1" || v == "true");
        let markers = SandboxMarkers::from_env();
        if should_auto_start_detached(auto_start_disabled, &markers)? {
            start_daemon_detached_with_config(config, timeout)
        } else {
            Ok(config)
        }
    }
}

pub(super) fn daemon_startup_is_blocked(config: &DaemonConfig) -> bool {
    if let Some(parent) = config.lock_path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }

    match LockFile::try_acquire(&config.lock_path) {
        Some(lock) => {
            drop(lock);
            false
        }
        None => true,
    }
}

pub(crate) fn daemon_is_up(config: &DaemonConfig) -> bool {
    #[cfg(not(windows))]
    {
        if !config.control_socket_path.exists() || !config.trace_socket_path.exists() {
            return false;
        }
    }
    let probe_timeout = Duration::from_millis(100);
    let control_ok = send_control_request_with_timeout(
        &config.control_socket_path,
        &ControlRequest::Ping,
        probe_timeout,
    )
    .is_ok();
    control_ok
        && local_socket_connects_with_timeout(&config.trace_socket_path, probe_timeout).is_ok()
}

#[cfg(any(windows, not(any(test, feature = "test-support"))))]
pub(super) fn wait_for_daemon_up(config: &DaemonConfig, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if daemon_is_up(config) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(not(any(test, feature = "test-support")))]
pub(super) fn start_daemon_detached_with_config(
    config: DaemonConfig,
    timeout: Duration,
) -> Result<DaemonConfig, String> {
    if daemon_is_up(&config) {
        return Ok(config);
    }

    remove_stale_daemon_files(&config);

    if daemon_startup_is_blocked(&config) {
        return Err(format!(
            "daemon startup blocked: lock held at {}",
            config.lock_path.display()
        ));
    }

    spawn_daemon_run_detached(&config)?;
    if wait_for_daemon_up(&config, timeout) {
        return Ok(config);
    }

    Err(format!(
        "timed out after {:?} waiting for daemon sockets {} and {}",
        timeout,
        config.control_socket_path.display(),
        config.trace_socket_path.display()
    ))
}

pub(super) fn daemon_runtime_dir(config: &DaemonConfig) -> Result<PathBuf, String> {
    config.ensure_parent_dirs().map_err(|e| e.to_string())?;
    config
        .lock_path
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "daemon lock path has no parent".to_string())
}

#[cfg(windows)]
pub(super) fn powershell_single_quote_literal(value: &OsStr) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "''"))
}

#[cfg(any(windows, not(any(test, feature = "test-support"))))]
pub(super) fn spawn_daemon_run_detached(config: &DaemonConfig) -> Result<(), String> {
    // Use current_git_ai_exe() instead of current_exe() to resolve through
    // symlinks. When the current exe is the git shim (e.g. ~/.local/bin/git),
    // current_exe() would spawn `git daemon run` which re-enters handle_git()
    // instead of handle_git_ai(), causing a fork bomb in async mode.
    let exe = crate::cli::git_ai_exe::current_git_ai_exe().map_err(|e| e.to_string())?;
    let runtime_dir = daemon_runtime_dir(config)?;

    #[cfg(windows)]
    {
        let script = format!(
            "Start-Process -FilePath {} -ArgumentList @('bg','run') -WorkingDirectory {} -WindowStyle Hidden",
            powershell_single_quote_literal(exe.as_os_str()),
            powershell_single_quote_literal(Path::new(&runtime_dir).as_os_str())
        );
        let mut child = Command::new("powershell.exe");
        child
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-WindowStyle")
            .arg("Hidden")
            .arg("-Command")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::operations::daemon::sanitize_daemon_child_environment(&mut child);
        child.env_remove("GIT_AI");
        let preferred_flags =
            CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB;
        child.creation_flags(preferred_flags);
        match child.spawn() {
            Ok(_) => Ok(()),
            Err(preferred_err) => {
                tracing::debug!(target: super::TRACING_TARGET,
                    "detached daemon spawn with CREATE_BREAKAWAY_FROM_JOB failed, retrying without it: {}",
                    preferred_err
                );
                child.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
                child.spawn().map(|_| ()).map_err(|fallback_err| {
                    format!(
                        "failed to spawn detached daemon with flags {:#x}: {}; retry without CREATE_BREAKAWAY_FROM_JOB also failed: {}",
                        preferred_flags, preferred_err, fallback_err
                    )
                })
            }
        }
    }

    #[cfg(not(windows))]
    {
        let mut child = Command::new(exe);
        child
            .arg("bg")
            .arg("run")
            .current_dir(&runtime_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::operations::daemon::sanitize_daemon_child_environment(&mut child);
        child.env_remove("GIT_AI");
        child.spawn().map(|_| ()).map_err(|e| e.to_string())
    }
}

#[cfg(not(windows))]
pub(super) fn spawn_daemon_run_with_piped_stderr(
    config: &DaemonConfig,
) -> Result<std::process::Child, String> {
    let exe = crate::cli::git_ai_exe::current_git_ai_exe().map_err(|e| e.to_string())?;
    let runtime_dir = daemon_runtime_dir(config)?;
    let mut child = Command::new(exe);
    child
        .arg("bg")
        .arg("run")
        .current_dir(&runtime_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    crate::operations::daemon::sanitize_daemon_child_environment(&mut child);
    child.env_remove("GIT_AI");
    child.spawn().map_err(|e| e.to_string())
}
