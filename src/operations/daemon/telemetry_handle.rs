//! Global daemon telemetry handle for sending events over the control socket.
//!
//! When daemon mode is active, this handle is initialized once on process start
//! and used by the observability and metrics modules to route events through the
//! daemon instead of writing to per-PID log files.
//!
//! The handle maintains a persistent socket connection that is shared across all
//! callers (telemetry, CAS, and potentially checkpoints). This avoids the
//! overhead of opening a new connection for every fire-and-forget event.

use crate::error::GitAiError;
use crate::model::daemon_control::{ControlRequest, ControlResponse};
use crate::model::telemetry::TelemetryEnvelope;
use crate::operations::daemon::client_helpers::{
    read_daemon_client_line, write_all_daemon_client_stream,
};
use crate::operations::daemon::{
    DaemonClientStream, open_local_socket_stream_with_timeout, set_daemon_client_stream_timeouts,
};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Read/write timeout for the persistent daemon socket.
/// Prevents indefinite blocking if the daemon becomes unresponsive.
const DAEMON_SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(2);

/// Maximum time to wait for the daemon socket on process start.
#[cfg(not(any(test, feature = "test-support")))]
const DAEMON_TELEMETRY_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Maximum time to wait when reopening a dropped connection.
const DAEMON_TELEMETRY_RECONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// Global handle to the daemon control socket for telemetry submission.
static DAEMON_TELEMETRY_HANDLE: OnceLock<Mutex<Option<DaemonTelemetryHandle>>> = OnceLock::new();

struct DaemonTelemetryHandle {
    socket_path: PathBuf,
    /// `None` after a failed exchange; the next request reconnects.
    conn: Option<BufReader<DaemonClientStream>>,
}

/// Open the daemon socket with read/write timeouts applied so that I/O never
/// blocks indefinitely (which would hold the global mutex and stall the entire
/// process).
fn connect(
    socket_path: &Path,
    timeout: Duration,
) -> Result<BufReader<DaemonClientStream>, GitAiError> {
    let mut stream = open_local_socket_stream_with_timeout(socket_path, timeout)?;
    let _ = set_daemon_client_stream_timeouts(&mut stream, socket_path, DAEMON_SOCKET_IO_TIMEOUT);
    Ok(BufReader::new(stream))
}

impl DaemonTelemetryHandle {
    fn new(socket_path: PathBuf, timeout: Duration) -> Result<Self, GitAiError> {
        let conn = connect(&socket_path, timeout)?;
        Ok(Self {
            socket_path,
            conn: Some(conn),
        })
    }

    /// Send a control request over the persistent connection and read the response.
    fn send(&mut self, request: &ControlRequest) -> Result<ControlResponse, String> {
        let mut body = serde_json::to_vec(request).map_err(|e| e.to_string())?;
        body.push(b'\n');
        // A failed write means the connection is dead (e.g. the daemon restarted),
        // so resend once on a fresh connection.
        let mut conn = match self.write(&body) {
            Ok(conn) => conn,
            Err(first_err) => self
                .write(&body)
                .map_err(|e| format!("send failed ({first_err}), retry failed ({e})"))?,
        };
        // The request was delivered, so it is never resent: a duplicate would
        // repeat the telemetry or flush. On failure the connection is dropped so
        // a late response cannot be read as the reply to the next request.
        let line = read_daemon_client_line(&mut conn, &self.socket_path, DAEMON_SOCKET_IO_TIMEOUT)
            .map_err(|e| e.to_string())?;
        self.conn = Some(conn);
        if line.trim().is_empty() {
            return Err("empty response from daemon".to_string());
        }
        serde_json::from_str(line.trim()).map_err(|e| format!("parse: {}", e))
    }

    /// Write `body` on the current connection, reconnecting if there is none.
    /// The connection is dropped when the write fails.
    fn write(&mut self, body: &[u8]) -> Result<BufReader<DaemonClientStream>, GitAiError> {
        let mut conn = match self.conn.take() {
            Some(conn) => conn,
            None => connect(&self.socket_path, DAEMON_TELEMETRY_RECONNECT_TIMEOUT)?,
        };
        write_all_daemon_client_stream(conn.get_mut(), &self.socket_path, body)?;
        Ok(conn)
    }
}

/// Result of attempting to initialize the global daemon telemetry handle.
pub enum DaemonTelemetryInitResult {
    /// Successfully connected to daemon.
    Connected,
    /// Failed to connect; contains the error message.
    Failed(String),
    /// Not in daemon mode or already inside the daemon process.
    Skipped,
}

/// Initialize the global daemon telemetry handle.
///
/// Should be called once on process start when daemon mode is active.
/// Attempts to connect to the daemon control socket (starting the daemon if needed)
/// with a 2-second timeout. The connection is kept open and reused for all
/// subsequent telemetry and CAS submissions.
///
/// Returns the result indicating success, failure, or skip.
pub fn init_daemon_telemetry_handle() -> DaemonTelemetryInitResult {
    // Don't initialize if we're inside the daemon process itself
    if crate::operations::daemon::daemon_process_active() {
        let _ = DAEMON_TELEMETRY_HANDLE.get_or_init(|| Mutex::new(None));
        return DaemonTelemetryInitResult::Skipped;
    }

    // In test builds, only connect if the daemon control socket is explicitly set.
    #[cfg(any(test, feature = "test-support"))]
    {
        let socket_path = std::env::var("GIT_AI_DAEMON_CONTROL_SOCKET")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from)
            .filter(|p| p.exists());

        match socket_path {
            Some(path) => install_handle(DaemonTelemetryHandle::new(path, Duration::from_secs(2))),
            None => {
                let _ = DAEMON_TELEMETRY_HANDLE.get_or_init(|| Mutex::new(None));
                DaemonTelemetryInitResult::Skipped
            }
        }
    }

    #[cfg(not(any(test, feature = "test-support")))]
    {
        // Try to ensure daemon is running and connect
        let config = match crate::operations::commands::daemon::ensure_daemon_running(
            DAEMON_TELEMETRY_CONNECT_TIMEOUT,
        ) {
            Ok(config) => config,
            Err(e) => {
                let _ = DAEMON_TELEMETRY_HANDLE.get_or_init(|| Mutex::new(None));
                return DaemonTelemetryInitResult::Failed(e);
            }
        };

        // Open a persistent connection to the control socket
        install_handle(DaemonTelemetryHandle::new(
            config.control_socket_path,
            DAEMON_TELEMETRY_CONNECT_TIMEOUT,
        ))
    }
}

fn install_handle(handle: Result<DaemonTelemetryHandle, GitAiError>) -> DaemonTelemetryInitResult {
    match handle {
        Ok(handle) => {
            let _ = DAEMON_TELEMETRY_HANDLE.get_or_init(|| Mutex::new(Some(handle)));
            DaemonTelemetryInitResult::Connected
        }
        Err(e) => {
            let _ = DAEMON_TELEMETRY_HANDLE.get_or_init(|| Mutex::new(None));
            DaemonTelemetryInitResult::Failed(e.to_string())
        }
    }
}

/// Check if the daemon telemetry handle is available for sending events.
pub fn daemon_telemetry_available() -> bool {
    DAEMON_TELEMETRY_HANDLE
        .get()
        .and_then(|m| m.lock().ok())
        .is_some_and(|guard| guard.is_some())
}

/// Send a control request over the shared persistent connection.
///
/// This is the unified entry point used by telemetry, CAS submissions,
/// and any other code that needs to talk to the daemon. The connection
/// is reused across calls; if the socket is dead it will reconnect.
///
/// Returns the daemon's response, or an error string on failure.
pub fn send_via_daemon(request: &ControlRequest) -> Result<ControlResponse, String> {
    let Some(handle_mutex) = DAEMON_TELEMETRY_HANDLE.get() else {
        return Err("daemon telemetry handle not initialized".to_string());
    };
    let Ok(mut guard) = handle_mutex.lock() else {
        return Err("daemon telemetry handle lock poisoned".to_string());
    };
    let Some(handle) = guard.as_mut() else {
        return Err("daemon telemetry handle not connected".to_string());
    };
    handle.send(request)
}

/// Submit telemetry envelopes to the daemon over the control socket.
///
/// Fire-and-forget: sends the request but doesn't propagate errors
/// (silently drops on failure since telemetry is best-effort).
pub fn submit_telemetry(envelopes: Vec<TelemetryEnvelope>) {
    if envelopes.is_empty() {
        return;
    }
    let request = ControlRequest::SubmitTelemetry { envelopes };
    let _ = send_via_daemon(&request);
}

/// Signal the daemon that new notes are pending in `notes-db` and should be
/// flushed to the remote backend.
///
/// Fire-and-forget: silently drops on failure (flush will happen on the next
/// periodic tick regardless).
pub fn submit_notes() {
    let request = ControlRequest::FlushNotes;
    let _ = send_via_daemon(&request);
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn response_timeout_does_not_resend_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("control.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        let received = Arc::new(AtomicUsize::new(0));
        let server_received = received.clone();
        // A daemon that accepts requests but never answers.
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let received = server_received.clone();
                std::thread::spawn(move || {
                    for _ in std::io::BufReader::new(stream)
                        .lines()
                        .map_while(Result::ok)
                    {
                        received.fetch_add(1, Ordering::SeqCst);
                    }
                });
            }
        });

        let mut handle = DaemonTelemetryHandle::new(socket_path, Duration::from_secs(1)).unwrap();
        assert!(handle.send(&ControlRequest::FlushNotes).is_err());
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(received.load(Ordering::SeqCst), 1);
    }
}
