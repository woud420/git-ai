use super::*;
use repos::test_file::ExpectedLineExt;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn assert_transport_timeout(command: &str, minimum_connections: usize) {
    let repo = TestRepo::new();
    let mut file = repo.filename("transport.txt");
    file.set_contents(lines!["preserved AI".ai()]);
    repo.stage_all_and_commit("Seed notes for stalled transport")
        .unwrap();
    file.assert_committed_lines(lines!["preserved AI".ai()]);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let remote = format!("http://{}/stalled.git", listener.local_addr().unwrap());
    repo.git_og(&["remote", "add", "stalled", &remote]).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let server = std::thread::spawn(move || {
        let mut connections = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !worker_stop.load(Ordering::Relaxed) && Instant::now() < deadline {
            match listener.accept() {
                Ok((connection, _)) => connections.push(connection),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("mock transport accept failed: {error}"),
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        connections.len()
    });
    let request = r#"{"remote_name":"stalled"}"#;
    let mut child = repo.git_ai_command_without_pre_sync_for_test(
        &[command, "--json", request],
        &[
            ("GIT_AI_TEST_NOTES_SYNC_TIMEOUT_MS", "250"),
            ("http_proxy", ""),
            ("HTTP_PROXY", ""),
            ("ALL_PROXY", ""),
            ("all_proxy", ""),
            ("NO_PROXY", "127.0.0.1"),
        ],
    );
    let start = Instant::now();
    let output = child.output().expect("internal notes command must start");
    let elapsed = start.elapsed();
    stop.store(true, Ordering::Relaxed);
    let connections = server.join().unwrap();
    assert!(
        elapsed < Duration::from_secs(3),
        "{command} waited {elapsed:?} for the stalled remote"
    );
    assert!(!output.status.success(), "{output:?}");
    let error: serde_json::Value = serde_json::from_slice(&output.stderr)
        .expect("the machine command must emit a structured error");
    assert!(
        error["error"].as_str().unwrap().contains("timed out"),
        "{output:?}"
    );
    assert!(
        connections >= minimum_connections,
        "expected fetch/push transport attempts, observed {connections}"
    );
    file.assert_committed_lines(lines!["preserved AI".ai()]);
}

#[test]
fn stalled_notes_fetch_returns_a_timeout_without_changing_local_attribution() {
    assert_transport_timeout("fetch-authorship-notes", 1);
}

#[test]
fn stalled_notes_prefetch_and_push_both_have_deadlines() {
    assert_transport_timeout("push-authorship-notes", 2);
}
