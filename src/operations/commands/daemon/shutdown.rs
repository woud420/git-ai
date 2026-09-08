use super::arguments::has_flag;
use super::startup::{
    daemon_config_from_env_or_default_paths, daemon_is_up, daemon_startup_is_blocked,
    daemon_startup_timeout, ensure_daemon_running_attached,
};
use crate::model::repository::lock_file::LockFile;
use crate::operations::commands::daemon_start_policy::{
    SandboxMarkers, require_detached_start_allowed,
};
use crate::operations::daemon::{
    ControlRequest, DaemonConfig, read_daemon_pid, send_control_request,
    send_control_request_fire_and_forget as send_nowait,
};
#[cfg(windows)]
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

/// Timeout for graceful shutdown before a hard kill during restart.
pub(super) const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn handle_shutdown(args: &[String]) -> Result<(), String> {
    let config = daemon_config_from_env_or_default_paths()?;
    if has_flag(args, "--hard") {
        if !daemon_is_up(&config) && !daemon_startup_is_blocked(&config) {
            return Err("background service is not running".to_string());
        }
        hard_kill_daemon(&config)
    } else {
        soft_shutdown_daemon(&config)
    }
}

pub(super) fn handle_restart(args: &[String]) -> Result<(), String> {
    let config = daemon_config_from_env_or_default_paths()?;
    let hard = has_flag(args, "--hard");

    // Check post-restart policy before shutting down a healthy daemon.
    require_detached_start_allowed(&SandboxMarkers::from_env())?;

    // Only attempt shutdown if daemon appears to be running.
    let was_running = daemon_is_up(&config) || daemon_startup_is_blocked(&config);
    if was_running {
        // Read the PID before shutdown so we can verify the process actually dies.
        let old_pid = read_daemon_pid(&config).ok();

        if hard {
            hard_kill_daemon(&config)?;
        } else {
            // Attempt soft shutdown; escalate to hard kill on timeout.
            let _ = send_nowait(&config.control_socket_path, &ControlRequest::Shutdown);
            if !wait_for_daemon_dead(&config, GRACEFUL_SHUTDOWN_TIMEOUT) {
                eprintln!("graceful shutdown timed out, force-killing daemon");
                hard_kill_daemon(&config)?;
            }
        }

        // Even after lock+sockets are gone, the process may still be alive
        // (e.g. tokio runtime draining blocking tasks). Verify and force-kill.
        if let Some(pid) = old_pid {
            wait_for_process_exit(pid, Duration::from_secs(2));
        }
    }

    // Start a fresh daemon.
    ensure_daemon_running_attached(daemon_startup_timeout()).map(|_| ())
}

pub(super) fn soft_shutdown_daemon(config: &DaemonConfig) -> Result<(), String> {
    let response = send_control_request(&config.control_socket_path, &ControlRequest::Shutdown)
        .map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response).map_err(|e| e.to_string())?
    );
    Ok(())
}

#[cfg(unix)]
pub(super) fn hard_kill_daemon(config: &DaemonConfig) -> Result<(), String> {
    let pid = read_daemon_pid(config).map_err(|e| format!("cannot read daemon pid: {}", e))?;
    let ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            // Process already dead — not an error.
            return Ok(());
        }
        return Err(format!("kill -9 {} failed: {}", pid, err));
    }
    // Wait briefly for the OS to reap the process and release the lock.
    let _ = wait_for_daemon_dead(config, Duration::from_secs(2));
    Ok(())
}

#[cfg(windows)]
pub(super) fn hard_kill_daemon(config: &DaemonConfig) -> Result<(), String> {
    let pid = read_daemon_pid(config).map_err(|e| format!("cannot read daemon pid: {}", e))?;
    let output = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .output()
        .map_err(|e| format!("failed to run taskkill: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Process already dead is not an error.
        if !stderr.contains("not found") {
            return Err(format!(
                "taskkill /F /T /PID {} failed: {}",
                pid,
                stderr.trim()
            ));
        }
    }
    let _ = wait_for_daemon_dead(config, Duration::from_secs(2));
    Ok(())
}

pub(super) fn wait_for_daemon_dead(config: &DaemonConfig, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let sockets_down = !daemon_is_up(config);
        let lock_free = LockFile::try_acquire(&config.lock_path)
            .map(|l| {
                drop(l);
                true
            })
            .unwrap_or(false);
        if sockets_down && lock_free {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Wait for a process to exit, force-killing it if it doesn't die within timeout.
/// This handles the case where the daemon lock/sockets are gone but the process
/// is still alive (e.g. tokio runtime draining blocking tasks).
///
/// Note: relies on PID liveness only. Theoretically susceptible to PID reuse if
/// the process is reaped and the PID recycled within the timeout window, but on
/// macOS/Linux with ~100k PID space this is not a realistic concern.
#[cfg(unix)]
pub(super) fn wait_for_process_exit(pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if ret != 0 {
            return; // Process is dead
        }
        if Instant::now() >= deadline {
            // Process still alive after timeout — force kill
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(windows)]
pub(super) fn wait_for_process_exit(_pid: u32, _timeout: Duration) {
    // On Windows, hard_kill_daemon uses taskkill /F which is synchronous.
}
