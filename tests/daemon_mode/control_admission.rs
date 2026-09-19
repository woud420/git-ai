use super::*;
use std::io::{BufRead, BufReader};

#[test]
fn control_connection_limit_rejects_excess_clients_and_recovers_after_close() {
    let repo = TestRepo::new_dedicated_daemon();
    let socket = daemon_control_socket_path(&repo);
    let mut clients = Vec::new();
    for _ in 0..32 {
        let stream = open_local_socket_stream_with_timeout(&socket, Duration::from_secs(2))
            .expect("connect within the control handler budget");
        let mut client = BufReader::new(stream);
        client
            .get_mut()
            .write_all(b"{\"method\":\"ping\"}\n")
            .unwrap();
        client.get_mut().flush().unwrap();
        let mut response = String::new();
        client.read_line(&mut response).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&response).unwrap()["ok"],
            json!(true)
        );
        clients.push(client);
    }
    let excess =
        send_control_request_with_timeout(&socket, &ControlRequest::Ping, Duration::from_secs(2));
    drop(clients);
    assert!(
        excess.is_err(),
        "a thirty-third live handler was admitted: {excess:?}"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if send_control_request_with_timeout(
            &socket,
            &ControlRequest::Ping,
            Duration::from_millis(250),
        )
        .is_ok_and(|response| response.ok)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "closed clients did not release admission capacity"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn daemon_health(socket: &Path) -> Value {
    send_control_request_with_timeout(
        socket,
        &ControlRequest::StatusDaemon,
        Duration::from_secs(2),
    )
    .unwrap()
    .data
    .unwrap()
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn overloaded_checkpoint_delivery_preserves_fences_and_replays_the_durable_suffix() {
    use git_ai::model::checkpoint_delivery::CheckpointDelivery;
    use git_ai::model::checkpoint_request::{
        BaseCommit, CheckpointFile, CheckpointRequest, PreparedPathRole,
    };
    use git_ai::model::repository::checkpoint_outbox::publish_delivery;
    use git_ai::model::working_log::AgentId;
    use git_ai::operations::commands::checkpoint_agent::delivery::{
        LiveFallbackClass, deliver_checkpoint_batch,
    };
    let temp = tempfile::tempdir().unwrap();
    let outbox = temp.path().join("outbox");
    let repo = TestRepo::new_with_daemon_env(&[
        (
            "GIT_AI_CHECKPOINT_OUTBOX_DIR",
            outbox.as_path().to_str().unwrap(),
        ),
        ("GIT_AI_TEST_OUTBOX_POLL_MS", "100"),
    ]);
    for index in 0..18 {
        let path = format!("file-{index}.txt");
        fs::write(repo.path().join(&path), "base\n").unwrap();
        repo.git_ai(&["checkpoint", "mock_known_human", &path])
            .unwrap();
    }
    let base = repo.stage_all_and_commit("base").unwrap().commit_sha;
    for index in 0..18 {
        repo.filename(&format!("file-{index}.txt"))
            .assert_committed_lines(lines!["base".human()]);
    }
    let deliveries = CheckpointDelivery::from_requests(
        (0..18)
            .map(|index| {
                let path = repo.path().join(format!("file-{index}.txt"));
                fs::write(&path, "base\nAI addition\n").unwrap();
                CheckpointRequest {
                    trace_id: format!("admission-{index}"),
                    checkpoint_kind: CheckpointKind::AiAgent,
                    agent_id: Some(AgentId {
                        tool: "mock_ai".into(),
                        id: format!("agent-{index}"),
                        model: "test".into(),
                    }),
                    files: vec![CheckpointFile {
                        path,
                        content: Some("base\nAI addition\n".into()),
                        repo_work_dir: repo.path().to_path_buf(),
                        base_commit: BaseCommit::Sha(base.clone()),
                    }],
                    path_role: PreparedPathRole::Edited,
                    stream_source: None,
                    metadata: std::collections::HashMap::new(),
                    delivery_id: None,
                }
            })
            .collect(),
    );
    let sid = "bounded-checkpoint-admission";
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let socket = daemon_control_socket_path(&repo);
    let mut root = open_local_socket_stream_with_timeout(
        &daemon_trace_socket_path(&repo),
        Duration::from_secs(2),
    )
    .unwrap();
    write_trace_frames_to_stream(
        &mut root,
        &[
            json!({"event":"start", "sid":sid, "argv":["git","commit","-m","held commit"], "time_ns":started}),
            json!({"event":"def_repo", "sid":sid, "repo":1, "worktree":repo.path().to_string_lossy(), "time_ns":started+1}),
        ],
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while daemon_health(&socket)["trace_roots_open_mutating"] != json!(1) {
        assert!(Instant::now() < deadline, "root fence was not registered");
        thread::sleep(Duration::from_millis(10));
    }
    let (tx, rx) = mpsc::channel();
    let handles: Vec<_> = deliveries[..16]
        .iter()
        .map(|delivery| {
            let delivery = delivery.clone();
            let socket = socket.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let response = send_control_request_with_timeout(
                    &socket,
                    &ControlRequest::CheckpointDeliver {
                        delivery: Box::new(delivery),
                    },
                    Duration::from_secs(30),
                );
                tx.send(response).unwrap();
            })
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    while daemon_health(&socket)["checkpoints_outstanding"] != json!(16) {
        assert!(
            rx.try_recv().is_err(),
            "checkpoint acknowledged before its fence closed"
        );
        assert!(
            Instant::now() < deadline,
            "sixteen requests did not reach their fence"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        send_control_request_with_timeout(&socket, &ControlRequest::Ping, Duration::from_secs(2))
            .unwrap()
            .ok
    );
    let legacy = send_control_request_with_timeout(
        &socket,
        &ControlRequest::CheckpointRun {
            request: Box::new(deliveries[16].request.clone()),
        },
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(
        !legacy.ok,
        "legacy checkpoint RPCs must share the live admission limit"
    );
    assert_eq!(daemon_health(&socket)["checkpoints_outstanding"], json!(16));
    let report = deliver_checkpoint_batch(
        &deliveries[16..],
        |delivery| {
            send_control_request_with_timeout(
                &socket,
                &ControlRequest::CheckpointDeliver {
                    delivery: Box::new(delivery.clone()),
                },
                Duration::from_secs(2),
            )
        },
        |delivery| {
            publish_delivery(outbox.as_path(), delivery)
                .inspect_err(|error| eprintln!("fixture publication failed: {error:?}"))
                .map(|_| ())
        },
    );
    let ready_before_release = ready_records(outbox.as_path()).len();
    write_trace_frames_to_stream(
        &mut root,
        &[json!({"event":"exit", "sid":sid, "code":1, "time_ns":started+2})],
    );
    drop(root);
    for handle in handles {
        handle.join().unwrap();
    }
    for _ in 0..16 {
        assert!(
            rx.recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap()
                .ok
        );
    }
    assert_eq!(report.acknowledged, 0);
    assert_eq!(report.published, 2);
    assert!(report.publication_failures.is_empty());
    assert_eq!(
        ready_before_release, 2,
        "unacknowledged deliveries must remain durable"
    );
    assert_eq!(
        report.live_fallback.unwrap().class,
        LiveFallbackClass::Rejected,
        "live overload must reject before enqueueing, rather than timing out after admission"
    );
    wait_for_ready_records_to_drain(outbox.as_path());
    repo.sync_daemon_force();
    let checkpoints = repo.current_working_logs().read_all_checkpoints().unwrap();
    for delivery in &deliveries {
        assert_eq!(
            checkpoints
                .iter()
                .filter(
                    |checkpoint| checkpoint.delivery_id.as_deref() == Some(&delivery.delivery_id)
                )
                .count(),
            1
        );
    }
    publish_delivery(outbox.as_path(), &deliveries[16]).unwrap();
    wait_for_ready_records_to_drain(outbox.as_path());
    assert_eq!(
        repo.current_working_logs()
            .read_all_checkpoints()
            .unwrap()
            .len(),
        checkpoints.len()
    );
    repo.stage_all_and_commit("commit live and replayed AI")
        .unwrap();
    for index in 0..18 {
        repo.filename(&format!("file-{index}.txt"))
            .assert_committed_lines(lines!["base".human(), "AI addition".ai()]);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn ready_records(root: &Path) -> Vec<std::path::PathBuf> {
    fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "ready")
        })
        .collect()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_for_ready_records_to_drain(root: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready_records(root).is_empty() {
        assert!(
            Instant::now() < deadline,
            "durable checkpoint suffix did not replay"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
