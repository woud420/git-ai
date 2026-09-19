use super::*;
use std::io::BufRead;
use std::sync::{Arc, mpsc};
use std::thread;

#[test]
fn ping_stalls_alert_once_and_rearm_only_after_acknowledgment() {
    let start = Instant::now();
    let mut health = PingHealth::new(Duration::from_secs(120));
    assert_eq!(health.observe(false, start), None);
    assert_eq!(
        health.observe(false, start + Duration::from_secs(119)),
        None
    );
    assert_eq!(
        health.observe(false, start + Duration::from_secs(120)),
        Some(Duration::from_secs(120))
    );
    assert_eq!(
        health.observe(false, start + Duration::from_secs(300)),
        None
    );
    assert_eq!(health.observe(true, start + Duration::from_secs(310)), None);
    assert_eq!(
        health.observe(false, start + Duration::from_secs(311)),
        None
    );
    assert_eq!(
        health.observe(false, start + Duration::from_secs(431)),
        Some(Duration::from_secs(120))
    );
}

#[test]
fn disabled_ping_health_does_not_accumulate_stalls() {
    let start = Instant::now();
    let mut health = PingHealth::new(Duration::ZERO);
    assert_eq!(health.observe(false, start), None);
    assert_eq!(
        health.observe(false, start + Duration::from_secs(1000)),
        None
    );
}

#[tokio::test]
async fn sentinel_acknowledgment_does_not_register_or_enqueue_a_git_root() {
    let coordinator = Arc::new(ActorDaemonCoordinator::new());
    let mut roots = std::collections::BTreeSet::new();
    for _ in 0..2 {
        super::super::process_trace_connection_line(
            "{\"event\":\"git_ai_health_ping\"}",
            coordinator.clone(),
            &mut roots,
        )
        .unwrap();
    }
    assert_eq!(
        coordinator
            .trace_health_pings_received
            .load(Ordering::Relaxed),
        2
    );
    assert!(roots.is_empty());
    assert_eq!(coordinator.queued_trace_payloads.load(Ordering::Relaxed), 0);
    assert_eq!(coordinator.next_trace_ingest_seq.load(Ordering::Relaxed), 0);
    coordinator.request_shutdown();
}

trait Duplex: Read + Write {}
impl<T: Read + Write> Duplex for T {}

fn mock_peer(
    action: impl FnOnce(&mut dyn Duplex) + Send + 'static,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    thread::JoinHandle<()>,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = super::super::DaemonConfig::from_home(dir.path()).control_socket_path;
    #[cfg(unix)]
    let listener = {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        listener
    };
    #[cfg(windows)]
    let listener = super::super::windows_pipe_connecting_server(&path, true).unwrap();
    let worker = thread::spawn(move || {
        #[cfg(unix)]
        let mut stream = {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("mock peer did not accept a client: {error}"),
                }
            }
        };
        #[cfg(windows)]
        let mut stream = listener
            .wait_ms(5000)
            .unwrap()
            .expect("mock peer did not accept a client");
        #[cfg(unix)]
        {
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
        }
        #[cfg(windows)]
        {
            stream.set_read_timeout(Some(Duration::from_secs(2)));
            stream.set_write_timeout(Some(Duration::from_secs(2)));
        }
        action(&mut stream);
    });
    (dir, path, worker)
}

#[test]
fn control_ping_requires_a_valid_bounded_response() {
    for (response, expected) in [
        (b"{\"ok\":true}\n".to_vec(), Some(true)),
        (b"{\"ok\":false}\n".to_vec(), Some(false)),
        (b"not-json\n".to_vec(), None),
        (vec![b'x'; MAX_PING_RESPONSE_BYTES], None),
    ] {
        let (_dir, path, worker) = mock_peer(move |stream| {
            let mut request = String::new();
            std::io::BufReader::new(&mut *stream)
                .read_line(&mut request)
                .unwrap();
            assert!(matches!(
                serde_json::from_str::<ControlRequest>(&request).unwrap(),
                ControlRequest::Ping
            ));
            stream.write_all(&response).unwrap();
        });
        let result = control_ping(&path);
        worker.join().unwrap();
        assert_eq!(result.ok().map(|response| response.ok), expected);
    }
}

#[test]
fn a_connected_peer_that_never_reads_cannot_block_health_pings() {
    let (release_tx, release_rx) = mpsc::channel();
    let (_dir, path, worker) = mock_peer(move |_| {
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let (result_tx, result_rx) = mpsc::channel();
    let probe = thread::spawn(move || result_tx.send(control_ping(&path)).unwrap());
    let result = result_rx.recv_timeout(Duration::from_secs(2));
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    probe.join().unwrap();
    let error = result
        .expect("connected but unread pipe must still honor its deadline")
        .expect_err("unresponsive peer must not acknowledge a ping");
    assert!(
        matches!(error, GitAiError::IoError(error) if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock))
    );
}
