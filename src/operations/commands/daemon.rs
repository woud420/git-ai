mod arguments;
mod logs;
mod shutdown;
mod startup;
#[path = "daemon_status.rs"]
mod status;
use crate::operations::commands::daemon_start_policy::{
    SandboxMarkers, require_detached_start_allowed,
};
use crate::operations::daemon::{
    ControlRequest, DaemonConfig, local_socket_connects_with_timeout,
    send_control_request_fire_and_forget as send_nowait,
};
use arguments::{default_repo_path, has_flag, is_help, parse_repo_arg};
use logs::handle_tail;
use shutdown::{
    GRACEFUL_SHUTDOWN_TIMEOUT, handle_restart, handle_shutdown, hard_kill_daemon,
    wait_for_daemon_dead,
};
use startup::{
    daemon_config_from_env_or_default_paths, daemon_runtime_dir, daemon_startup_is_blocked,
    handle_start,
};
pub(crate) use startup::{daemon_is_up, ensure_daemon_running};
use std::time::Duration;
#[cfg(windows)]
const TRACING_TARGET: &str = module_path!();

pub fn handle_daemon(args: &[String]) {
    if args.is_empty() || is_help(args[0].as_str()) {
        print_help();
        std::process::exit(0);
    }

    match args[0].as_str() {
        "start" => {
            if let Err(e) = handle_start(&args[1..]) {
                eprintln!("Failed to start: {}", e);
                std::process::exit(1);
            }
        }
        "run" => {
            if let Err(e) = handle_run(&args[1..]) {
                eprintln!("Failed to run: {}", e);
                std::process::exit(1);
            }
        }
        "status" => {
            let repo = parse_repo_arg(&args[1..]).unwrap_or_else(default_repo_path);
            if let Err(e) = status::handle_status(repo) {
                eprintln!("Failed to get status: {}", e);
                std::process::exit(1);
            }
        }
        "shutdown" => {
            if let Err(e) = handle_shutdown(&args[1..]) {
                eprintln!("Failed to shut down: {}", e);
                std::process::exit(1);
            }
        }
        "restart" => {
            if let Err(e) = handle_restart(&args[1..]) {
                eprintln!("Failed to restart: {}", e);
                std::process::exit(1);
            }
        }
        "tail" => {
            if let Err(e) = handle_tail(&args[1..]) {
                eprintln!("Failed to tail log: {}", e);
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("Unknown subcommand: {}", args[0]);
            print_help();
            std::process::exit(1);
        }
    }
}

fn handle_run(args: &[String]) -> Result<(), String> {
    if has_flag(args, "--mode") {
        return Err("--mode is no longer supported; daemon always runs in write mode".to_string());
    }
    crate::tokio_runtime::configure_daemon_allocator()?;
    let config = daemon_config_from_env_or_default_paths()?;
    let markers = SandboxMarkers::from_env();
    if let Some(marker) = markers.strong_marker_name() {
        eprintln!(
            "warning: running a foreground daemon with sandbox marker {marker}; detached auto-start remains blocked"
        );
    }
    let runtime_dir = daemon_runtime_dir(&config)?;
    std::env::set_current_dir(&runtime_dir).map_err(|e| {
        format!(
            "failed to set daemon runtime cwd to {}: {}",
            runtime_dir.display(),
            e
        )
    })?;
    let runtime = crate::tokio_runtime::build_daemon_runtime()?;
    crate::tokio_runtime::initialize();
    let exit_action = runtime
        .block_on(async move { crate::operations::daemon::run_daemon(config).await })
        .map_err(|e| e.to_string())?;

    match exit_action {
        crate::operations::daemon::DaemonExitAction::Stop => {}
        crate::operations::daemon::DaemonExitAction::Restart => {
            ensure_daemon_running(Duration::from_secs(5)).map(|_| ())?;
        }
        crate::operations::daemon::DaemonExitAction::RestartAfterUpdate => {
            // Daemon is fully dead (lock released, sockets removed, threads joined).
            // Now safe to self-update — if the install cannot proceed, bring the
            // daemon back so a failed update does not leave the service down.
            match crate::operations::daemon::daemon_run_pending_self_update() {
                crate::operations::daemon::DaemonSelfUpdateOutcome::Installed => {
                    #[cfg(not(windows))]
                    {
                        ensure_daemon_running(Duration::from_secs(5)).map(|_| ())?;
                    }
                }
                crate::operations::daemon::DaemonSelfUpdateOutcome::NoUpdate
                | crate::operations::daemon::DaemonSelfUpdateOutcome::Failed => {
                    ensure_daemon_running(Duration::from_secs(5)).map(|_| ())?;
                }
            }
        }
    }

    Ok(())
}

fn print_help() {
    eprintln!("git-ai bg - run and control git-ai background service");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  git-ai bg start");
    eprintln!("  git-ai bg run");
    eprintln!("  git-ai bg status [--repo <path>]");
    eprintln!("  git-ai bg shutdown [--hard]");
    eprintln!("  git-ai bg restart [--hard]");
    eprintln!("  git-ai bg tail [-n <lines>] [--full] [-f | --follow]");
}

/// Shut down the running daemon (soft then hard) and wait for it to fully exit.
/// Used by internal callers (install-hooks, upgrade) that need the daemon stopped
/// before proceeding.
pub(crate) fn stop_daemon(config: &DaemonConfig, timeout: Duration) -> Result<(), String> {
    // Nothing to do if daemon isn't running.
    if !daemon_is_up(config) && !daemon_startup_is_blocked(config) {
        return Ok(());
    }

    // Attempt soft shutdown via control socket if reachable.
    if local_socket_connects_with_timeout(&config.control_socket_path, Duration::from_millis(100))
        .is_ok()
    {
        let _ = send_nowait(&config.control_socket_path, &ControlRequest::Shutdown);
    }

    if wait_for_daemon_dead(config, timeout) {
        return Ok(());
    }

    // Soft shutdown didn't work — escalate.
    hard_kill_daemon(config)
}

/// Shut down the running daemon and start a fresh one. Escalates to hard kill
/// if the soft shutdown doesn't complete within GRACEFUL_SHUTDOWN_TIMEOUT.
pub(crate) fn restart_daemon(config: &DaemonConfig) -> Result<(), String> {
    require_detached_start_allowed(&SandboxMarkers::from_env())?;
    let was_running = daemon_is_up(config) || daemon_startup_is_blocked(config);
    if was_running {
        stop_daemon(config, GRACEFUL_SHUTDOWN_TIMEOUT)?;
    }
    ensure_daemon_running(Duration::from_secs(5)).map(|_| ())
}
