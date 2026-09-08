use super::{
    daemon_command_output, daemon_control_socket_path, shutdown_daemon, wait_for_daemon_sockets,
};
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::operations::daemon::control_api::CasSyncPayload;
#[cfg(unix)]
use git_ai::operations::daemon::send_control_request_with_timeout;
use git_ai::operations::daemon::{
    ControlRequest, ControlResponse, TelemetryEnvelope, open_local_socket_stream_with_timeout,
};
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::thread;
use std::time::Duration;

#[cfg(unix)]
#[test]
fn control_request_protocol_preserves_framing_and_errors() {
    use git_ai::error::GitAiError;
    const TIMEOUT: Duration = Duration::from_millis(30);

    fn exchange(response: Option<(&'static str, u64)>) -> Result<ControlResponse, GitAiError> {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("control.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; b"{\"method\":\"ping\"}\n".len()];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"{\"method\":\"ping\"}\n");
            if let Some((body, delay)) = response {
                thread::sleep(Duration::from_millis(delay));
                let _ = stream.write_all(body.as_bytes());
            }
        });
        let result = send_control_request_with_timeout(&socket, &ControlRequest::Ping, TIMEOUT);
        server.join().unwrap();
        result
    }

    let missing = tempfile::tempdir().unwrap().path().join("missing");
    let connect_error = send_control_request_with_timeout(&missing, &ControlRequest::Ping, TIMEOUT)
        .unwrap_err()
        .to_string();
    assert!(connect_error.contains("timed out after 30ms connecting daemon socket"));

    assert!(exchange(Some(("{\"ok\":true}\n", 0))).unwrap().ok);
    let malformed = exchange(Some(("{\n", 0)));
    assert!(matches!(malformed, Err(GitAiError::JsonError(_))));
    let error = |response| exchange(response).unwrap_err().to_string();
    assert_eq!(
        error(Some(("\n", 0))),
        "Generic error: empty daemon control response"
    );
    assert!(error(None).contains("closed without a response"));
    assert!(
        error(Some(("{\"ok\":true}\n", 80)))
            .contains("timed out after 30ms reading daemon response")
    );
}

/// Helper: send a ControlRequest over an existing buffered stream and read one response line.
fn send_on_persistent_conn<R: Read + Write>(
    reader: &mut BufReader<R>,
    request: &ControlRequest,
) -> ControlResponse {
    let mut body = serde_json::to_vec(request).expect("serialize request");
    body.push(b'\n');
    reader
        .get_mut()
        .write_all(&body)
        .expect("write request to daemon");
    reader.get_mut().flush().expect("flush request");
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read response from daemon");
    assert!(
        !line.trim().is_empty(),
        "daemon response should not be empty"
    );
    serde_json::from_str::<ControlResponse>(line.trim()).expect("parse daemon response")
}

/// Integration test: verifies that a persistent control socket connection can
/// deliver telemetry envelopes and CAS payloads to the daemon, and that the
/// daemon acknowledges each request with `ok: true` without closing the
/// connection between requests.
#[test]
fn daemon_telemetry_and_cas_over_persistent_connection() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);

    // Start the daemon
    let start_output = daemon_command_output(&repo, &["bg", "start"], repo.path());
    assert!(
        start_output.status.success(),
        "daemon start should succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&start_output.stdout),
        String::from_utf8_lossy(&start_output.stderr)
    );
    wait_for_daemon_sockets(&repo);

    // Open a single persistent connection (mirrors the shared handle in telemetry_handle.rs)
    let control_path = daemon_control_socket_path(&repo);
    let stream = open_local_socket_stream_with_timeout(&control_path, Duration::from_secs(2))
        .expect("should connect to daemon control socket");
    let mut reader = BufReader::new(stream);

    // 1. Send telemetry envelopes (Message + Error variants)
    let telemetry_req = ControlRequest::SubmitTelemetry {
        envelopes: vec![
            TelemetryEnvelope::Message {
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                message: "integration test message".to_string(),
                level: "info".to_string(),
                context: None,
            },
            TelemetryEnvelope::Error {
                timestamp: "2026-01-01T00:00:01Z".to_string(),
                message: "integration test error event".to_string(),
                context: None,
            },
        ],
    };
    let resp = send_on_persistent_conn(&mut reader, &telemetry_req);
    assert!(resp.ok, "telemetry submit should succeed: {:?}", resp.error);

    // 2. Send CAS payloads over the *same* connection
    let cas_req = ControlRequest::SubmitCas {
        records: vec![
            CasSyncPayload {
                hash: "abc123".to_string(),
                data: "test cas data".to_string(),
                metadata: None,
            },
            CasSyncPayload {
                hash: "def456".to_string(),
                data: "more cas data".to_string(),
                metadata: Some("test-meta".to_string()),
            },
        ],
    };
    let resp = send_on_persistent_conn(&mut reader, &cas_req);
    assert!(resp.ok, "CAS submit should succeed: {:?}", resp.error);

    // 3. Send another batch of telemetry to confirm the connection stays alive
    let telemetry_req2 = ControlRequest::SubmitTelemetry {
        envelopes: vec![TelemetryEnvelope::Error {
            timestamp: "2026-01-01T00:00:02Z".to_string(),
            message: "integration test error".to_string(),
            context: None,
        }],
    };
    let resp = send_on_persistent_conn(&mut reader, &telemetry_req2);
    assert!(
        resp.ok,
        "second telemetry submit should succeed on persistent connection: {:?}",
        resp.error
    );

    // 4. Verify the daemon is still healthy via a status request on the same conn
    let status_req = ControlRequest::StatusFamily {
        repo_working_dir: repo.canonical_path().to_string_lossy().to_string(),
    };
    let resp = send_on_persistent_conn(&mut reader, &status_req);
    assert!(
        resp.ok,
        "status request should succeed on persistent connection: {:?}",
        resp.error
    );

    // Clean up
    drop(reader);
    shutdown_daemon(&repo);
}
