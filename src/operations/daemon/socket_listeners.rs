#[allow(unused_imports)]
use super::*;
use crate::error::GitAiError;
use serde_json::Value;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::sync::Arc;

// platform-specific imports
#[cfg(not(windows))]
use interprocess::local_socket::{ListenerOptions, prelude::*};
#[cfg(windows)]
use named_pipe::{
    ConnectingServer as WindowsConnectingServer, OpenMode as WindowsPipeOpenMode,
    PipeClient as WindowsPipeClient, PipeOptions as WindowsPipeOptions,
    PipeServer as WindowsPipeServer,
};
#[cfg(windows)]
use std::path::Path;

/// Text-dedup constructor for the control/trace worker-panicked thread-joins
/// below; Display output is unchanged from the sites it replaces.
#[cfg(windows)]
fn worker_panicked_error(what: &str) -> GitAiError {
    GitAiError::Generic(format!("daemon {} worker panicked", what))
}

pub fn control_listener_loop_actor(
    control_socket_path: PathBuf,
    coordinator: Arc<ActorDaemonCoordinator>,
    runtime_handle: tokio::runtime::Handle,
) -> Result<(), GitAiError> {
    #[cfg(not(windows))]
    {
        remove_socket_if_exists(&control_socket_path)?;
        let listener = ListenerOptions::new()
            .name(local_socket_name(&control_socket_path)?)
            .create_sync()
            .map_err(|e| GitAiError::Generic(format!("failed binding control socket: {}", e)))?;
        set_socket_owner_only(&control_socket_path)?;
        for stream in listener.incoming() {
            if coordinator.is_shutting_down() {
                break;
            }
            let Ok(stream) = stream else {
                continue;
            };
            let coord = coordinator.clone();
            let handle = runtime_handle.clone();
            if std::thread::Builder::new()
                .spawn(move || {
                    if let Err(e) = handle_control_connection_actor(stream, coord, handle) {
                        tracing::debug!(%e, "control connection error");
                    }
                })
                .is_err()
            {
                tracing::error!("control listener: failed to spawn handler thread");
                break;
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        let mut workers = Vec::new();
        let worker_count = windows_control_pipe_worker_count();
        let first_connecting = windows_pipe_connecting_server(&control_socket_path, true)?;
        {
            let path = control_socket_path.clone();
            let coord = coordinator.clone();
            let handle = runtime_handle.clone();
            workers.push(std::thread::spawn(move || {
                let result =
                    windows_control_pipe_worker_loop(path, first_connecting, coord.clone(), handle);
                if let Err(error) = &result {
                    tracing::error!(%error, "control worker error");
                    coord.request_shutdown();
                }
                result
            }));
        }
        for _ in 1..worker_count {
            let path = control_socket_path.clone();
            let coord = coordinator.clone();
            let handle = runtime_handle.clone();
            let connecting = windows_pipe_connecting_server(&path, false)?;
            workers.push(std::thread::spawn(move || {
                let result =
                    windows_control_pipe_worker_loop(path, connecting, coord.clone(), handle);
                if let Err(error) = &result {
                    tracing::error!(%error, "control worker error");
                    coord.request_shutdown();
                }
                result
            }));
        }

        while !coordinator.is_shutting_down() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        wake_windows_pipe_workers(&control_socket_path, worker_count);

        for worker in workers {
            let result = worker
                .join()
                .map_err(|_| worker_panicked_error("control"))?;
            result?;
        }

        Ok(())
    }
}

#[cfg(windows)]
pub fn windows_pipe_connecting_server(
    pipe_path: &Path,
    first_instance: bool,
) -> Result<WindowsConnectingServer, GitAiError> {
    let mut options = WindowsPipeOptions::new(pipe_path.as_os_str());
    options
        .first(first_instance)
        .open_mode(WindowsPipeOpenMode::Duplex);
    options.single().map_err(|e| {
        GitAiError::Generic(format!(
            "failed binding windows daemon pipe {}: {}",
            pipe_path.display(),
            e
        ))
    })
}

#[cfg(windows)]
pub fn windows_trace_pipe_worker_count() -> usize {
    #[cfg(feature = "test-support")]
    if let Ok(raw) = std::env::var("GIT_AI_TEST_WINDOWS_TRACE_PIPE_WORKERS")
        && let Ok(count) = raw.parse::<usize>()
        && count > 0
    {
        return count;
    }

    WINDOWS_TRACE_PIPE_WORKERS
}

#[cfg(windows)]
pub fn windows_control_pipe_worker_count() -> usize {
    #[cfg(feature = "test-support")]
    if let Ok(raw) = std::env::var("GIT_AI_TEST_WINDOWS_CONTROL_PIPE_WORKERS")
        && let Ok(count) = raw.parse::<usize>()
        && count > 0
    {
        return count;
    }

    WINDOWS_CONTROL_PIPE_WORKERS
}

#[cfg(windows)]
pub fn wake_windows_pipe_workers(pipe_path: &Path, worker_count: usize) {
    for _ in 0..worker_count {
        let _ = WindowsPipeClient::connect_ms(pipe_path.as_os_str(), 100);
    }
}

#[cfg(windows)]
pub fn windows_control_pipe_worker_loop(
    control_socket_path: PathBuf,
    mut connecting: WindowsConnectingServer,
    coordinator: Arc<ActorDaemonCoordinator>,
    runtime_handle: tokio::runtime::Handle,
) -> Result<(), GitAiError> {
    loop {
        let server = connecting.wait().map_err(|e| {
            GitAiError::Generic(format!(
                "failed accepting control pipe {}: {}",
                control_socket_path.display(),
                e
            ))
        })?;

        if coordinator.is_shutting_down() {
            let _ = server.disconnect();
            break;
        }

        connecting = windows_pipe_connecting_server(&control_socket_path, false)?;

        let coord = coordinator.clone();
        let handle = runtime_handle.clone();
        std::thread::Builder::new()
            .spawn(move || {
                handle_windows_control_pipe_connection(server, coord, handle);
            })
            .map_err(|e| {
                GitAiError::Generic(format!(
                    "failed spawning control pipe handler for {}: {}",
                    control_socket_path.display(),
                    e
                ))
            })?;
    }

    Ok(())
}

pub fn trace_listener_loop_actor(
    trace_socket_path: PathBuf,
    coordinator: Arc<ActorDaemonCoordinator>,
) -> Result<(), GitAiError> {
    #[cfg(not(windows))]
    {
        remove_socket_if_exists(&trace_socket_path)?;
        let listener = ListenerOptions::new()
            .name(local_socket_name(&trace_socket_path)?)
            .create_sync()
            .map_err(|e| GitAiError::Generic(format!("failed binding trace socket: {}", e)))?;
        set_socket_owner_only(&trace_socket_path)?;
        for stream in listener.incoming() {
            if coordinator.is_shutting_down() {
                break;
            }
            let Ok(stream) = stream else {
                continue;
            };
            // Raise the receive buffer on each accepted connection. Unlike TCP,
            // a Unix-domain listener's SO_RCVBUF is not inherited by accepted
            // connections, so this per-connection call is what takes effect.
            if let Err(error) = set_trace_socket_recv_buffer(&stream) {
                tracing::debug!(%error, "trace connection recv buffer setup failed");
            }
            if let Err(error) = coordinator.trace_unidentified_connection_opened() {
                tracing::debug!(%error, "trace connection open bookkeeping error");
                continue;
            }
            if let Err(error) =
                stream.set_recv_timeout(Some(TRACE_CONNECTION_BOOTSTRAP_READ_TIMEOUT))
            {
                tracing::debug!(%error, "trace connection bootstrap timeout setup failed");
            }
            let mut reader = BufReader::new(stream);
            let mut observed_roots = std::collections::BTreeSet::new();
            match bootstrap_trace_connection_actor_reader(
                &mut reader,
                coordinator.clone(),
                &mut observed_roots,
            ) {
                Ok(TraceConnectionBootstrap::Eof) => {
                    if let Err(error) =
                        finalize_trace_connection_roots(coordinator.clone(), observed_roots)
                    {
                        tracing::debug!(
                            %error,
                            "trace connection close bookkeeping error"
                        );
                    }
                    continue;
                }
                Ok(TraceConnectionBootstrap::Stop) => {
                    if let Err(error) =
                        finalize_trace_connection_roots(coordinator.clone(), observed_roots)
                    {
                        tracing::debug!(
                            %error,
                            "trace connection close bookkeeping error"
                        );
                    }
                    continue;
                }
                Ok(TraceConnectionBootstrap::Continue) => {}
                Err(error) => {
                    tracing::debug!(%error, "trace connection bootstrap error");
                    if let Err(error) =
                        finalize_trace_connection_roots(coordinator.clone(), observed_roots)
                    {
                        tracing::debug!(
                            %error,
                            "trace connection close bookkeeping error"
                        );
                    }
                    continue;
                }
            }
            if let Err(error) = reader.get_ref().set_recv_timeout(None) {
                tracing::debug!(%error, "trace connection bootstrap timeout clear failed");
            }
            #[cfg(feature = "test-support")]
            if let Ok(raw_delay_ms) =
                std::env::var("GIT_AI_TEST_TRACE_LISTENER_WORKER_SPAWN_DELAY_MS")
                && let Ok(delay_ms) = raw_delay_ms.parse::<u64>()
                && delay_ms > 0
            {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            let coord = coordinator.clone();
            let observed_roots_on_spawn_failure = observed_roots.clone();
            if std::thread::Builder::new()
                .spawn(move || {
                    if let Err(e) =
                        handle_trace_connection_actor_reader(reader, coord, observed_roots)
                    {
                        tracing::debug!(%e, "trace connection error");
                    }
                })
                .is_err()
            {
                tracing::error!("trace listener: failed to spawn handler thread");
                if let Err(error) = finalize_trace_connection_roots(
                    coordinator.clone(),
                    observed_roots_on_spawn_failure,
                ) {
                    tracing::debug!(
                        %error,
                        "trace connection close bookkeeping error"
                    );
                }
                break;
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        let mut workers = Vec::new();
        let worker_count = windows_trace_pipe_worker_count();
        let first_connecting = windows_pipe_connecting_server(&trace_socket_path, true)?;
        {
            let path = trace_socket_path.clone();
            let coord = coordinator.clone();
            workers.push(std::thread::spawn(move || {
                let result = windows_trace_pipe_worker_loop(path, first_connecting, coord.clone());
                if let Err(error) = &result {
                    tracing::error!(%error, "trace worker error");
                    coord.request_shutdown();
                }
                result
            }));
        }
        for _ in 1..worker_count {
            let path = trace_socket_path.clone();
            let coord = coordinator.clone();
            let connecting = windows_pipe_connecting_server(&path, false)?;
            workers.push(std::thread::spawn(move || {
                let result = windows_trace_pipe_worker_loop(path, connecting, coord.clone());
                if let Err(error) = &result {
                    tracing::error!(%error, "trace worker error");
                    coord.request_shutdown();
                }
                result
            }));
        }

        while !coordinator.is_shutting_down() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        wake_windows_pipe_workers(&trace_socket_path, worker_count);

        for worker in workers {
            let result = worker.join().map_err(|_| worker_panicked_error("trace"))?;
            result?;
        }

        Ok(())
    }
}

#[cfg(windows)]
pub fn windows_trace_pipe_worker_loop(
    trace_socket_path: PathBuf,
    mut connecting: WindowsConnectingServer,
    coordinator: Arc<ActorDaemonCoordinator>,
) -> Result<(), GitAiError> {
    loop {
        let server = connecting.wait().map_err(|e| {
            GitAiError::Generic(format!(
                "failed accepting trace pipe {}: {}",
                trace_socket_path.display(),
                e
            ))
        })?;

        if coordinator.is_shutting_down() {
            let _ = server.disconnect();
            break;
        }

        connecting = windows_pipe_connecting_server(&trace_socket_path, false)?;

        let coord = coordinator.clone();
        std::thread::Builder::new()
            .spawn(move || {
                handle_windows_trace_pipe_connection(server, coord);
            })
            .map_err(|e| {
                GitAiError::Generic(format!(
                    "failed spawning trace pipe handler for {}: {}",
                    trace_socket_path.display(),
                    e
                ))
            })?;
    }

    Ok(())
}

#[cfg(windows)]
pub fn handle_windows_trace_pipe_connection(
    mut server: WindowsPipeServer,
    coordinator: Arc<ActorDaemonCoordinator>,
) {
    if let Err(e) = coordinator.trace_unidentified_connection_opened() {
        tracing::debug!(%e, "trace connection open bookkeeping error");
        return;
    }
    let reader = BufReader::new(&mut server);
    if let Err(e) =
        handle_trace_connection_actor_reader(reader, coordinator, std::collections::BTreeSet::new())
    {
        tracing::debug!(%e, "trace connection error");
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn handle_trace_connection_actor(
    stream: LocalSocketStream,
    coordinator: Arc<ActorDaemonCoordinator>,
) -> Result<(), GitAiError> {
    coordinator.trace_unidentified_connection_opened()?;
    let reader = BufReader::new(stream);
    handle_trace_connection_actor_reader(reader, coordinator, std::collections::BTreeSet::new())
}

#[cfg(not(windows))]
pub enum TraceConnectionBootstrap {
    Continue,
    Stop,
    Eof,
}

pub struct TraceLineOutcome {
    continue_reading: bool,
    #[cfg(not(windows))]
    bootstrap_complete: bool,
}

#[cfg(not(windows))]
pub const TRACE_CONNECTION_BOOTSTRAP_MAX_LINES: usize = 8;

#[cfg(not(windows))]
pub fn bootstrap_trace_connection_actor_reader<R: Read>(
    reader: &mut BufReader<R>,
    coordinator: Arc<ActorDaemonCoordinator>,
    observed_roots: &mut std::collections::BTreeSet<String>,
) -> Result<TraceConnectionBootstrap, GitAiError> {
    for _ in 0..TRACE_CONNECTION_BOOTSTRAP_MAX_LINES {
        let line = match read_json_line(reader) {
            Ok(Some(line)) => line,
            Ok(None) => return Ok(TraceConnectionBootstrap::Eof),
            Err(error) if trace_bootstrap_read_timed_out(&error) => {
                return Ok(TraceConnectionBootstrap::Continue);
            }
            Err(error) => return Err(error),
        };
        let Some(outcome) =
            process_trace_connection_line(&line, coordinator.clone(), observed_roots)?
        else {
            continue;
        };
        if !outcome.continue_reading {
            return Ok(TraceConnectionBootstrap::Stop);
        }
        if outcome.bootstrap_complete {
            return Ok(TraceConnectionBootstrap::Continue);
        }
    }
    Ok(TraceConnectionBootstrap::Continue)
}

#[cfg(not(windows))]
pub fn trace_bootstrap_read_timed_out(error: &GitAiError) -> bool {
    matches!(
        error,
        GitAiError::IoError(io_error)
            if matches!(
                io_error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            )
    )
}

pub fn handle_trace_connection_actor_reader<R: Read>(
    mut reader: BufReader<R>,
    coordinator: Arc<ActorDaemonCoordinator>,
    mut observed_roots: std::collections::BTreeSet<String>,
) -> Result<(), GitAiError> {
    while let Some(line) = read_json_line(&mut reader)? {
        if process_trace_connection_line(&line, coordinator.clone(), &mut observed_roots)?
            .is_some_and(|outcome| !outcome.continue_reading)
        {
            break;
        }
    }

    finalize_trace_connection_roots(coordinator, observed_roots)
}

pub fn process_trace_connection_line(
    line: &str,
    coordinator: Arc<ActorDaemonCoordinator>,
    observed_roots: &mut std::collections::BTreeSet<String>,
) -> Result<Option<TraceLineOutcome>, GitAiError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let mut parsed: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    #[cfg(not(windows))]
    let event = parsed
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    #[cfg(not(windows))]
    let mut bootstrap_complete = false;
    if let Some(sid) = parsed.get("sid").and_then(Value::as_str) {
        let was_unidentified = observed_roots.is_empty();
        let root_sid = trace_root_sid(sid).to_string();
        // `start` carries argv but not the worktree. Keep bootstrapping on the
        // listener thread until the root `def_repo` event has been processed;
        // that is the first point where trace augmentation can capture reflog
        // start offsets with a concrete worktree.
        #[cfg(not(windows))]
        if event == "def_repo" && sid == root_sid {
            bootstrap_complete = true;
        }
        if observed_roots.insert(root_sid.clone()) {
            let _ = coordinator.trace_root_connection_opened(&root_sid);
        }
        if was_unidentified {
            coordinator.trace_unidentified_connection_identified_or_closed()?;
        }
    }
    // Only enqueue payloads for mutating commands.  Read-only invocations
    // (status, diff, stash list, worktree list, …) are handled inline by
    // prepare_trace_payload_for_ingest and must not enter the serial ingest
    // queue — doing so causes the >1-minute backlog seen with IDEs that
    // issue dozens of read-only git commands per second.
    let continue_reading = !(coordinator.prepare_trace_payload_for_ingest(&mut parsed)
        && coordinator.enqueue_trace_payload(parsed).is_err());
    Ok(Some(TraceLineOutcome {
        continue_reading,
        #[cfg(not(windows))]
        bootstrap_complete,
    }))
}
