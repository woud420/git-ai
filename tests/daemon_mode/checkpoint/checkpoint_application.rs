use super::super::*;

#[test]
#[serial]
fn daemon_mode_post_commit_uploads_prompt_cas() {
    assert_post_commit_uploads_prompt_cas();
}

#[test]
#[cfg(windows)]
#[serial]
fn daemon_windows_stalled_checkpoint_clients_do_not_block_later_control_requests() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let mut daemon = DaemonGuard::start_with_env(
        &repo,
        &[
            ("GIT_AI_TEST_WINDOWS_CONTROL_PIPE_WORKERS", "2"),
            ("GIT_AI_DAEMON_UPDATE_CHECK_INTERVAL", "86400"),
            ("GIT_AI_DAEMON_MAX_UPTIME_SECS", "86400"),
        ],
    );
    let control_socket = daemon_control_socket_path(&repo);

    let mut stalled_clients = (0..2)
        .map(|_| {
            let mut command = Command::new(get_binary_path());
            command
                .args(["checkpoint", "codex", "--hook-input", "stdin"])
                .current_dir(repo.path())
                .env("GIT_AI_TEST_DB_PATH", repo.test_db_path())
                .env("GITAI_TEST_DB_PATH", repo.test_db_path())
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            configure_test_home_env(&mut command, repo.test_home_path());
            configure_test_daemon_env(
                &mut command,
                &repo.daemon_home_path(),
                &control_socket,
                &daemon_trace_socket_path(&repo),
            );
            command.spawn().expect("failed to spawn stalled checkpoint")
        })
        .collect::<Vec<_>>();
    thread::sleep(Duration::from_millis(250));

    let (response_tx, response_rx) = mpsc::channel();
    let request_socket = control_socket.clone();
    let request_repo = repo_workdir_string(&repo);
    thread::spawn(move || {
        let _ = response_tx.send(send_control_request(
            &request_socket,
            &ControlRequest::StatusFamily {
                repo_working_dir: request_repo,
            },
        ));
    });
    let response = response_rx.recv_timeout(Duration::from_secs(2));

    for client in &mut stalled_clients {
        let _ = client.kill();
        let _ = client.wait();
    }
    let response = response
        .expect("control request timed out after every original pipe worker was stalled")
        .expect("control request failed after every original pipe worker was stalled");
    assert!(
        response.ok,
        "later control request should return an ok response: {:?}",
        response
    );
    daemon.shutdown();
}

#[test]
#[serial]
fn daemon_write_mode_applies_delegated_checkpoint_and_updates_state() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::Dedicated);
    let completion_baseline = repo.daemon_total_completion_count();

    fs::write(repo.path().join("delegate-write.txt"), "base\n").expect("failed to write base");
    repo.git(&["add", "delegate-write.txt"])
        .expect("add should succeed");
    repo.stage_all_and_commit("base commit")
        .expect("base commit should succeed");

    fs::write(
        repo.path().join("delegate-write.txt"),
        "base\nwritten by delegated checkpoint\n",
    )
    .expect("failed to write updated file");

    repo.git_ai_with_env(
        &["checkpoint", "mock_ai", "delegate-write.txt"],
        &[("GIT_AI_DAEMON_CHECKPOINT_DELEGATE", "true")],
    )
    .expect("delegated checkpoint should succeed");

    wait_for_expected_top_level_completions(&repo, completion_baseline, 1);

    let checkpoints = repo
        .current_working_logs()
        .read_all_checkpoints()
        .expect("checkpoints should be readable");
    assert!(
        checkpoints
            .iter()
            .any(|checkpoint| checkpoint.kind == CheckpointKind::AiAgent),
        "write-mode daemon should execute checkpoint side effect"
    );
}

#[test]
#[serial]
fn daemon_test_mode_git_ai_checkpoint_runs_via_daemon() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::Dedicated);

    fs::write(repo.path().join("daemon-mode-checkpoint.txt"), "base\n")
        .expect("failed to write base");
    repo.git(&["add", "daemon-mode-checkpoint.txt"])
        .expect("add should succeed");
    repo.stage_all_and_commit("base commit")
        .expect("base commit should succeed");

    fs::write(
        repo.path().join("daemon-mode-checkpoint.txt"),
        "base\nchanged through daemon mode\n",
    )
    .expect("failed to write updated file");
    let completion_baseline = repo.daemon_total_completion_count();

    repo.git_ai(&["checkpoint", "mock_ai", "daemon-mode-checkpoint.txt"])
        .expect("daemon-mode checkpoint should succeed");

    repo.wait_for_next_daemon_checkpoint_completion(completion_baseline);

    let checkpoints = repo
        .current_working_logs()
        .read_all_checkpoints()
        .expect("checkpoints should be readable");
    assert!(
        checkpoints
            .iter()
            .any(|checkpoint| checkpoint.kind == CheckpointKind::AiAgent),
        "daemon-mode checkpoint should still write the ai_agent checkpoint side effect"
    );
}

#[test]
#[serial]
fn daemon_test_mode_human_checkpoint_with_explicit_preset_queues_via_daemon() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::Dedicated);

    fs::write(repo.path().join("human-direct-path.txt"), "base\n").expect("failed to write base");
    repo.git_og(&["add", "human-direct-path.txt"])
        .expect("add should succeed");
    repo.git_og(&["commit", "-m", "base commit"])
        .expect("base commit should succeed");

    fs::write(repo.path().join("human-direct-path.txt"), "base\nhuman\n")
        .expect("failed to write human change");
    let completion_baseline = repo.daemon_total_completion_count();

    repo.git_ai(&["checkpoint", "human", "human-direct-path.txt"])
        .expect("human checkpoint with preset should succeed");

    repo.wait_for_next_daemon_checkpoint_completion(completion_baseline);

    let git_ai_repo = git_ai::operations::git::repository::find_repository_in_path(
        repo.path()
            .to_str()
            .expect("repo path should be valid UTF-8"),
    )
    .expect("repository should still be discoverable");
    let base_commit = git_ai_repo
        .head()
        .ok()
        .and_then(|head| head.target().ok())
        .unwrap_or_else(|| "initial".to_string());
    let checkpoints = git_ai_repo
        .storage
        .working_log_for_base_commit(&base_commit)
        .unwrap()
        .read_all_checkpoints()
        .expect("checkpoints should be readable");
    assert!(
        checkpoints
            .iter()
            .any(|checkpoint| checkpoint.kind == CheckpointKind::Human),
        "human checkpoint should write the human checkpoint side effect"
    );
}
