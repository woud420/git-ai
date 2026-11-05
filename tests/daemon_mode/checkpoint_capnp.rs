use super::*;
#[cfg(not(windows))]
use git_ai::model::checkpoint_capnp::encode_delivery;
use git_ai::model::checkpoint_delivery::CheckpointDelivery;
use git_ai::model::checkpoint_request::{
    BaseCommit, CheckpointFile, CheckpointRequest, PreparedPathRole,
};
use git_ai::model::working_log::AgentId;
#[cfg(unix)]
use git_ai::operations::daemon::{CheckpointTransport, send_checkpoint_delivery_with_transport};
use git_ai::operations::daemon::{
    ControlResponse, DaemonClientStream, set_daemon_client_stream_timeouts,
};
use std::io::{BufRead, BufReader};

fn capnp_client(repo: &TestRepo) -> BufReader<DaemonClientStream> {
    let socket = daemon_control_socket_path(repo);
    let mut stream =
        open_local_socket_stream_with_timeout(&socket, Duration::from_secs(2)).unwrap();
    set_daemon_client_stream_timeouts(&mut stream, &socket, Duration::from_secs(5)).unwrap();
    BufReader::new(stream)
}

fn write_json<W: Write>(writer: &mut W, value: &Value) {
    serde_json::to_writer(&mut *writer, value).unwrap();
    writer.write_all(b"\n").unwrap();
    writer.flush().unwrap();
}

fn read_response<R: BufRead>(reader: &mut R) -> ControlResponse {
    let mut response = String::new();
    assert_ne!(reader.read_line(&mut response).unwrap(), 0);
    serde_json::from_str(&response).unwrap()
}

fn is_closed_body_connection(error: &std::io::Error) -> bool {
    #[cfg(windows)]
    let pipe_disconnected = error.raw_os_error()
        == Some(windows_sys::Win32::Foundation::ERROR_PIPE_NOT_CONNECTED as i32);
    #[cfg(not(windows))]
    let pipe_disconnected = false;
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::UnexpectedEof
    ) || pipe_disconnected
}

fn assert_body_connection_closed<R: Read>(reader: &mut R) {
    let mut byte = [0];
    match reader.read(&mut byte) {
        Ok(0) => {}
        Err(error) if is_closed_body_connection(&error) => {}
        result => panic!("pending binary body connection remained open: {result:?}"),
    }
}

fn negotiate<R: Read + Write>(client: &mut BufReader<R>, encoded_bytes: usize) -> ControlResponse {
    write_json(
        client.get_mut(),
        &json!({"method":"checkpoint.deliver.capnp.v1", "params":{"encoded_bytes":encoded_bytes}}),
    );
    read_response(client)
}

fn assert_ready(response: &ControlResponse) {
    assert!(response.ok, "negotiation failed: {response:?}");
    assert_eq!(
        response.data.as_ref().unwrap()["checkpoint_capnp_v1"],
        json!("ready")
    );
}

fn checkpoint_delivery(repo: &TestRepo, content: &str, base: &str) -> CheckpointDelivery {
    CheckpointDelivery::from_requests(vec![CheckpointRequest {
        trace_id: "capnp-checkpoint-transport".into(),
        checkpoint_kind: CheckpointKind::AiAgent,
        agent_id: Some(AgentId {
            tool: "mock_ai".into(),
            id: "capnp-test-agent".into(),
            model: "test".into(),
        }),
        files: vec![CheckpointFile {
            path: repo.path().join("captured.txt"),
            content: Some(content.into()),
            repo_work_dir: repo.path().to_path_buf(),
            base_commit: BaseCommit::Sha(base.into()),
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: std::collections::HashMap::new(),
        delivery_id: None,
    }])
    .remove(0)
}

#[test]
fn checkpoint_capnp_negotiation_acknowledges_before_reading_binary_body() {
    let repo = TestRepo::new_dedicated_daemon();
    let mut client = capnp_client(&repo);
    assert_ready(&negotiate(&mut client, 8));
}

#[test]
fn checkpoint_capnp_pending_body_does_not_prevent_daemon_shutdown() {
    let repo = TestRepo::new_dedicated_daemon();
    let mut client = capnp_client(&repo);
    assert_ready(&negotiate(&mut client, 8));
    let shutdown = send_control_request_with_timeout(
        &daemon_control_socket_path(&repo),
        &ControlRequest::Shutdown,
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(shutdown.ok, "shutdown failed: {shutdown:?}");
    assert_body_connection_closed(&mut client);
}

#[test]
fn checkpoint_capnp_idle_body_deadline_releases_shared_admission() {
    let repo = TestRepo::new_dedicated_daemon();
    let mut pending = Vec::new();
    for _ in 0..16 {
        let mut client = capnp_client(&repo);
        assert_ready(&negotiate(&mut client, 8));
        pending.push(client);
    }
    for client in &mut pending {
        assert_body_connection_closed(client);
    }
    let mut replacement = capnp_client(&repo);
    assert_ready(&negotiate(&mut replacement, 8));
}

#[test]
fn checkpoint_capnp_trickling_body_cannot_extend_receive_deadline() {
    let repo = TestRepo::new_dedicated_daemon();
    let mut client = capnp_client(&repo);
    assert_ready(&negotiate(&mut client, 64));
    let started = Instant::now();
    let mut closed = false;
    while started.elapsed() < Duration::from_secs(3) {
        thread::sleep(Duration::from_millis(80));
        match client.get_mut().write_all(&[0]) {
            Ok(()) => {}
            Err(error) if is_closed_body_connection(&error) => {
                closed = true;
                break;
            }
            Err(error) => panic!("unexpected trickle write failure: {error}"),
        }
    }
    assert!(
        closed,
        "trickling extended the overall body receive deadline"
    );
    let mut replacement = capnp_client(&repo);
    assert_ready(&negotiate(&mut replacement, 8));
}

#[test]
#[cfg(not(windows))]
fn checkpoint_capnp_mixed_socket_preserves_snapshots_attribution_and_delivery_identity() {
    let repo = TestRepo::new_dedicated_daemon();
    repo.write_file("captured.txt", "base\n");
    repo.git_ai(&["checkpoint", "mock_known_human", "captured.txt"])
        .unwrap();
    let base = repo
        .stage_all_and_commit("known human base")
        .unwrap()
        .commit_sha;
    repo.filename("captured.txt")
        .assert_committed_lines(lines!["base".human()]);
    let captured_line = "é雪🦀 \"quoted\" \\ source ".repeat(4096);
    let captured = format!("base\n{captured_line}\n");
    let delivery = checkpoint_delivery(&repo, &captured, &base);
    let encoded = encode_delivery(&delivery).unwrap();
    let working_log = repo.current_working_logs();
    repo.write_file("captured.txt", "base\nchanged after capture\n");

    let mut client = capnp_client(&repo);
    write_json(client.get_mut(), &json!({"method":"ping"}));
    assert!(read_response(&mut client).ok);
    assert_ready(&negotiate(&mut client, encoded.len()));
    client.get_mut().write_all(&encoded).unwrap();
    client.get_mut().flush().unwrap();
    assert!(read_response(&mut client).ok);
    assert_eq!(
        working_log
            .read_all_checkpoints()
            .unwrap()
            .iter()
            .filter(|checkpoint| {
                checkpoint.delivery_id.as_deref() == Some(delivery.delivery_id.as_str())
            })
            .count(),
        1,
        "the final response must acknowledge application, not only binary admission"
    );
    write_json(client.get_mut(), &json!({"method":"ping"}));
    assert!(read_response(&mut client).ok);
    write_json(
        client.get_mut(),
        &json!({"method":"checkpoint.deliver", "params":{"delivery":delivery}}),
    );
    assert!(read_response(&mut client).ok);
    assert_eq!(
        working_log
            .read_all_checkpoints()
            .unwrap()
            .iter()
            .filter(|checkpoint| {
                checkpoint.delivery_id.as_deref() == Some(delivery.delivery_id.as_str())
            })
            .count(),
        1,
        "JSON redelivery of a binary delivery must deduplicate"
    );
    drop(client);
    repo.write_file("captured.txt", &captured);
    repo.stage_all_and_commit("commit captured binary checkpoint")
        .unwrap();
    repo.filename("captured.txt")
        .assert_committed_lines(lines!["base".human(), captured_line.ai()]);
}

#[test]
fn checkpoint_capnp_rejects_invalid_lengths_before_binary_admission() {
    let repo = TestRepo::new_dedicated_daemon();
    for encoded_bytes in [0, 64 * 1024 * 1024 + 8, u32::MAX as usize] {
        let mut client = capnp_client(&repo);
        let rejected = negotiate(&mut client, encoded_bytes);
        assert!(!rejected.ok, "invalid length admitted: {encoded_bytes}");
        assert!(rejected.error.is_some());
    }
    assert!(
        send_control_request(&daemon_control_socket_path(&repo), &ControlRequest::Ping)
            .unwrap()
            .ok
    );
}

#[test]
#[cfg(unix)]
fn checkpoint_capnp_final_ack_waits_for_trace_fence_and_applied_checkpoint() {
    let repo = TestRepo::new_dedicated_daemon();
    repo.write_file("captured.txt", "base\n");
    let base = repo.stage_all_and_commit("base").unwrap().commit_sha;
    let working_log = repo.current_working_logs();
    let delivery = checkpoint_delivery(&repo, "base\nAI addition\n", &base);
    let socket = daemon_control_socket_path(&repo);
    let mut root = open_local_socket_stream_with_timeout(
        &daemon_trace_socket_path(&repo),
        Duration::from_secs(2),
    )
    .unwrap();
    let sid = "capnp-application-ack-fence";
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    write_trace_frames_to_stream(
        &mut root,
        &[
            json!({"event":"start", "sid":sid, "argv":["git","commit","-m","held root"], "time_ns":started}),
            json!({"event":"def_repo", "sid":sid, "repo":1, "worktree":repo.path().to_string_lossy(), "time_ns":started+1}),
        ],
    );
    let health = || {
        send_control_request_with_timeout(
            &socket,
            &ControlRequest::StatusDaemon,
            Duration::from_secs(2),
        )
        .unwrap()
        .data
        .unwrap()
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while health()["trace_roots_open_mutating"] != json!(1) {
        assert!(Instant::now() < deadline, "trace fence did not register");
        thread::sleep(Duration::from_millis(10));
    }
    let (tx, rx) = mpsc::channel();
    let client_socket = socket.clone();
    let client_delivery = delivery.clone();
    let client = thread::spawn(move || {
        tx.send(send_checkpoint_delivery_with_transport(
            &client_socket,
            &client_delivery,
            CheckpointTransport::Capnp,
        ))
        .unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while health()["checkpoints_outstanding"] != json!(1) {
        assert!(
            rx.try_recv().is_err(),
            "client treated admission as final application acknowledgement"
        );
        assert!(
            Instant::now() < deadline,
            "binary checkpoint did not reach trace fence"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "checkpoint acknowledged while earlier mutating root was open"
    );
    write_trace_frames_to_stream(
        &mut root,
        &[json!({"event":"exit", "sid":sid, "code":1, "time_ns":started+2})],
    );
    drop(root);
    assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap().ok);
    client.join().unwrap();
    assert_eq!(
        working_log
            .read_all_checkpoints()
            .unwrap()
            .iter()
            .filter(|checkpoint| checkpoint.delivery_id.as_deref()
                == Some(delivery.delivery_id.as_str()))
            .count(),
        1
    );
}

#[test]
fn checkpoint_capnp_malformed_body_is_rejected_and_daemon_remains_healthy() {
    let repo = TestRepo::new_dedicated_daemon();
    let mut client = capnp_client(&repo);
    assert_ready(&negotiate(&mut client, 8));
    client.get_mut().write_all(&[0xff; 8]).unwrap();
    client.get_mut().flush().unwrap();
    assert!(!read_response(&mut client).ok);
    drop(client);
    assert!(
        send_control_request(&daemon_control_socket_path(&repo), &ControlRequest::Ping)
            .unwrap()
            .ok
    );
}

#[test]
#[cfg(unix)]
fn checkpoint_capnp_truncated_body_is_rejected_and_daemon_remains_healthy() {
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;
    let repo = TestRepo::new_dedicated_daemon();
    let stream = UnixStream::connect(daemon_control_socket_path(&repo)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut client = BufReader::new(stream);
    assert_ready(&negotiate(&mut client, 16));
    client.get_mut().write_all(&[0; 8]).unwrap();
    client.get_mut().shutdown(Shutdown::Write).unwrap();
    // EOF during a body may close the client or produce an error response;
    // it must never acknowledge a partially received checkpoint.
    let mut response = String::new();
    let read = client.read_line(&mut response).unwrap();
    if read > 0 {
        assert!(
            !serde_json::from_str::<ControlResponse>(&response)
                .unwrap()
                .ok
        );
    }
    assert!(
        send_control_request(&daemon_control_socket_path(&repo), &ControlRequest::Ping)
            .unwrap()
            .ok
    );
}

#[test]
fn checkpoint_capnp_negotiation_shares_admission_with_json_and_releases_on_disconnect() {
    let repo = TestRepo::new_dedicated_daemon();
    let mut pending = Vec::new();
    for _ in 0..16 {
        let mut client = capnp_client(&repo);
        assert_ready(&negotiate(&mut client, 8));
        pending.push(client);
    }
    let mut rejected = capnp_client(&repo);
    let response = negotiate(&mut rejected, 8);
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("admission is full"));
    assert!(
        send_control_request(&daemon_control_socket_path(&repo), &ControlRequest::Ping)
            .unwrap()
            .ok
    );
    let delivery = checkpoint_delivery(
        &repo,
        "content\n",
        "0000000000000000000000000000000000000000",
    );
    let response = send_control_request(
        &daemon_control_socket_path(&repo),
        &ControlRequest::CheckpointDeliver {
            delivery: Box::new(delivery),
        },
    )
    .unwrap();
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("admission is full"));
    drop(pending);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = negotiate(&mut rejected, 8);
        if response.ok {
            assert_ready(&response);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "disconnect did not release checkpoint admission"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn accept_fake_daemon(
    listener: &std::os::unix::net::UnixListener,
) -> BufReader<std::os::unix::net::UnixStream> {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "client did not connect to fake daemon"
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("fake daemon accept failed: {error}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    BufReader::new(stream)
}

#[cfg(unix)]
fn read_json<R: BufRead>(reader: &mut R) -> Value {
    let mut request = String::new();
    assert_ne!(reader.read_line(&mut request).unwrap(), 0);
    serde_json::from_str(&request).unwrap()
}

#[test]
#[cfg(unix)]
fn checkpoint_capnp_old_daemon_fallback_sends_no_body_before_ready() {
    use std::os::unix::net::UnixListener;
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("old-daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let delivery = checkpoint_delivery(
        &repo,
        "captured\n",
        "0000000000000000000000000000000000000000",
    );
    let delivery_id = delivery.delivery_id.clone();
    let server = thread::spawn(move || {
        let mut peer = accept_fake_daemon(&listener);
        let header = read_json(&mut peer);
        assert_eq!(header["method"], json!("checkpoint.deliver.capnp.v1"));
        peer.get_mut()
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut byte = [0];
        let error = peer
            .read(&mut byte)
            .expect_err("client sent binary before receiving ready");
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ));
        peer.get_mut()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write_json(
            peer.get_mut(),
            &json!({"ok":false,"error":"invalid control request: unknown variant `checkpoint.deliver.capnp.v1`"}),
        );
        let legacy = read_json(&mut peer);
        assert_eq!(legacy["method"], json!("checkpoint.deliver"));
        assert_eq!(
            legacy["params"]["delivery"]["delivery_id"],
            json!(delivery_id)
        );
        write_json(peer.get_mut(), &json!({"ok":true}));
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    });
    let response =
        send_checkpoint_delivery_with_transport(&socket, &delivery, CheckpointTransport::Capnp)
            .unwrap();
    assert!(response.ok);
    server.join().unwrap();
}

#[test]
#[cfg(unix)]
fn checkpoint_capnp_application_rejection_after_body_never_retries_json() {
    use std::os::unix::net::UnixListener;
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("new-daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let delivery = checkpoint_delivery(
        &repo,
        "captured\n",
        "0000000000000000000000000000000000000000",
    );
    let server = thread::spawn(move || {
        let mut peer = accept_fake_daemon(&listener);
        let header = read_json(&mut peer);
        write_json(
            peer.get_mut(),
            &json!({"ok":true,"data":{"checkpoint_capnp_v1":"ready"}}),
        );
        let mut body = vec![0; header["params"]["encoded_bytes"].as_u64().unwrap() as usize];
        peer.read_exact(&mut body).unwrap();
        git_ai::model::checkpoint_capnp::decode_delivery(&body).unwrap();
        write_json(
            peer.get_mut(),
            &json!({"ok":false,"error":"application refused checkpoint"}),
        );
        let mut next = [0];
        assert_eq!(
            peer.read(&mut next).unwrap(),
            0,
            "binary delivery was retried after final rejection"
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    });
    let response =
        send_checkpoint_delivery_with_transport(&socket, &delivery, CheckpointTransport::Capnp)
            .unwrap();
    assert!(!response.ok);
    assert_eq!(
        response.error.as_deref(),
        Some("application refused checkpoint")
    );
    server.join().unwrap();
}

#[test]
#[cfg(unix)]
fn checkpoint_capnp_lost_application_ack_never_retries_json() {
    use std::os::unix::net::UnixListener;
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("lost-ack.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let delivery = checkpoint_delivery(
        &repo,
        "captured\n",
        "0000000000000000000000000000000000000000",
    );
    let server = thread::spawn(move || {
        let mut peer = accept_fake_daemon(&listener);
        let header = read_json(&mut peer);
        write_json(
            peer.get_mut(),
            &json!({"ok":true,"data":{"checkpoint_capnp_v1":"ready"}}),
        );
        let mut body = vec![0; header["params"]["encoded_bytes"].as_u64().unwrap() as usize];
        peer.read_exact(&mut body).unwrap();
        drop(peer);
        listener
    });
    let result =
        send_checkpoint_delivery_with_transport(&socket, &delivery, CheckpointTransport::Capnp);
    assert!(
        result.is_err(),
        "a lost final ACK must remain an unacknowledged delivery"
    );
    let listener = server.join().unwrap();
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "client retried a possibly applied binary delivery"
    );
}
