#![cfg(unix)]

use super::*;
use std::os::unix::fs::OpenOptionsExt;

struct BlockingConfig {
    path: PathBuf,
    contents: String,
}

impl BlockingConfig {
    fn new(repo: &TestRepo) -> Self {
        let path = repo.path().join(".git/config");
        let contents = format!(
            "{}\n[alias]\n\tinspect = status\n",
            fs::read_to_string(&path).unwrap()
        );
        fs::remove_file(&path).unwrap();
        let config = Self { path, contents };
        assert!(
            Command::new("mkfifo")
                .arg(&config.path)
                .status()
                .unwrap()
                .success()
        );
        config
    }

    fn wait_for_reader(&self) -> fs::File {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&self.path)
            {
                Ok(writer) => return writer,
                Err(error)
                    if error.raw_os_error() == Some(libc::ENXIO) && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("normalizer did not open the config FIFO: {error}"),
            }
        }
    }
}

impl Drop for BlockingConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        fs::write(&self.path, &self.contents).expect("restore repository config");
    }
}

#[test]
fn daemon_normalizer_config_read_does_not_block_control_runtime() {
    let repo = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_TEST_DAEMON_RUNTIME_WORKER_THREADS", "1"),
        ("GIT_AI_DAEMON_UPDATE_CHECK_INTERVAL", "86400"),
        ("GIT_AI_DAEMON_MAX_UPTIME_SECS", "86400"),
    ]);
    let config = BlockingConfig::new(&repo);
    let mut stream = open_local_socket_stream_with_timeout(
        &daemon_trace_socket_path(&repo),
        DAEMON_TEST_PROBE_TIMEOUT,
    )
    .unwrap();
    write_trace_frames_to_stream(
        &mut stream,
        &[
            json!({"event": "start", "sid": "blocked-alias", "argv": ["git", "inspect"], "time_ns": 1000}),
            json!({"event": "def_repo", "sid": "blocked-alias", "worktree": repo_workdir_string(&repo), "repo": repo.path().join(".git"), "time_ns": 1001}),
        ],
    );
    // A nonblocking FIFO writer opens only after the real config reader has
    // arrived. Keeping it open without bytes holds that reader in read_to_end.
    let mut writer = config.wait_for_reader();
    let response = send_control_request_with_timeout(
        &daemon_control_socket_path(&repo),
        &ControlRequest::StatusFamily {
            repo_working_dir: repo_workdir_string(&repo),
        },
        Duration::from_millis(500),
    );
    writer.write_all(config.contents.as_bytes()).unwrap();
    drop(writer);
    drop(config);
    assert!(
        response
            .expect("blocked normalization occupied the only async worker")
            .ok
    );

    write_trace_frames_to_stream(
        &mut stream,
        &[
            json!({"event": "exit", "sid": "blocked-alias", "code": 0, "time_ns": 1100}),
            trace_atexit_frame("blocked-alias", 0, 1101),
        ],
    );
    drop(stream);
    repo.sync_daemon();
    let mut file = repo.filename("after-normalization.txt");
    file.set_contents(lines!["AI content after the blocked read".ai()]);
    repo.stage_all_and_commit("Commit after normalization resumes")
        .unwrap();
    file.assert_committed_lines(lines!["AI content after the blocked read".ai()]);
    repo.shutdown_dedicated_daemon_for_test();
}
