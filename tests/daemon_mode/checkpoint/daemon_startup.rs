use super::super::*;

#[test]
#[serial]
fn daemon_start_spawns_detached_run_process() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);

    let mut command = Command::new(get_binary_path());
    command
        .arg("bg")
        .arg("start")
        .current_dir(repo.path())
        .env("GIT_AI_TEST_DB_PATH", repo.test_db_path())
        .env("GITAI_TEST_DB_PATH", repo.test_db_path());
    configure_test_home_env(&mut command, repo.test_home_path());
    configure_test_daemon_env(
        &mut command,
        &repo.daemon_home_path(),
        &daemon_control_socket_path(&repo),
        &daemon_trace_socket_path(&repo),
    );
    let output = command.output().expect("failed to invoke daemon start");
    assert!(
        output.status.success(),
        "daemon start should return success: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let mut status_ok = false;
    for _ in 0..80 {
        match send_control_request(
            &daemon_control_socket_path(&repo),
            &ControlRequest::StatusFamily {
                repo_working_dir: repo_workdir_string(&repo),
            },
        ) {
            Ok(response) if response.ok => {
                status_ok = true;
                break;
            }
            _ => {
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
    assert!(status_ok, "daemon should be reachable after `daemon start`");

    let _ = send_control_request(
        &daemon_control_socket_path(&repo),
        &ControlRequest::Shutdown,
    );
}

#[test]
#[serial]
fn daemon_start_refuses_sandbox_inherited_autostart() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);

    let mut command = Command::new(get_binary_path());
    command
        .arg("bg")
        .arg("start")
        .current_dir(repo.path())
        .env("GIT_AI_TEST_DB_PATH", repo.test_db_path())
        .env("GITAI_TEST_DB_PATH", repo.test_db_path())
        .env("SANDBOX_RUNTIME", "seatbelt");
    configure_test_home_env(&mut command, repo.test_home_path());
    configure_test_daemon_env(
        &mut command,
        &repo.daemon_home_path(),
        &daemon_control_socket_path(&repo),
        &daemon_trace_socket_path(&repo),
    );

    let output = command
        .output()
        .expect("failed to invoke daemon start inside a sandbox");
    let stderr = String::from_utf8_lossy(&output.stderr);
    if daemon_control_socket_path(&repo).exists() {
        let _ = send_control_request(
            &daemon_control_socket_path(&repo),
            &ControlRequest::Shutdown,
        );
    }
    assert!(
        !output.status.success(),
        "daemon start must refuse a sandbox-inherited detached daemon: stdout={} stderr={stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr.to_ascii_lowercase().contains("sandbox"),
        "daemon start should explain the sandbox refusal: stderr={stderr}"
    );
    assert!(
        !daemon_control_socket_path(&repo).exists(),
        "sandbox refusal must not leave a daemon control socket"
    );
}

#[test]
#[serial]
fn daemon_restart_refuses_sandbox_before_shutdown() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    start_daemon_for_repo(&repo);

    let mut command = Command::new(get_binary_path());
    command
        .arg("bg")
        .arg("restart")
        .current_dir(repo.path())
        .env("GIT_AI_TEST_DB_PATH", repo.test_db_path())
        .env("GITAI_TEST_DB_PATH", repo.test_db_path())
        .env("SANDBOX_RUNTIME", "seatbelt");
    configure_test_home_env(&mut command, repo.test_home_path());
    configure_test_daemon_env(
        &mut command,
        &repo.daemon_home_path(),
        &daemon_control_socket_path(&repo),
        &daemon_trace_socket_path(&repo),
    );

    let output = command
        .output()
        .expect("failed to invoke daemon restart inside a sandbox");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "daemon restart must refuse before shutting down a healthy daemon: stdout={} stderr={stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        send_control_request(
            &daemon_control_socket_path(&repo),
            &ControlRequest::StatusFamily {
                repo_working_dir: repo_workdir_string(&repo),
            },
        )
        .is_ok(),
        "sandbox restart refusal must leave the existing daemon running"
    );

    let _ = send_control_request(
        &daemon_control_socket_path(&repo),
        &ControlRequest::Shutdown,
    );
}

#[test]
#[serial]
fn daemon_run_allows_sandbox_marker_for_foreground_process() {
    // ENG-211: Explicit foreground startup remains available inside a sandbox.
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start_with_env(&repo, &[("SANDBOX_RUNTIME", "seatbelt")]);

    assert!(
        send_control_request(
            &daemon_control_socket_path(&repo),
            &ControlRequest::StatusFamily {
                repo_working_dir: repo_workdir_string(&repo),
            },
        )
        .is_ok(),
        "foreground daemon should remain usable when started with a sandbox marker"
    );
}

#[test]
#[should_panic(expected = "pending daemon sync work")]
fn dedicated_daemon_restart_rejects_pending_traced_command_for_test() {
    let mut repo = TestRepo::new_dedicated_daemon();

    repo.git(&["commit", "--allow-empty", "-m", "base"])
        .expect("base commit should succeed");
    repo.git(&["branch", "pending-before-restart"])
        .expect("branch creation should succeed");

    repo.restart_dedicated_daemon_for_test();
}

#[test]
#[serial]
fn checkpoint_delegate_autostarts_daemon_when_unavailable() {
    // Test builds disable daemon auto-spawning from ensure_daemon_running to
    // prevent process storms. We verify that checkpoint delegation works by
    // restarting the daemon manually before the checkpoint call.
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::Dedicated);

    fs::write(repo.path().join("delegate-fallback.txt"), "base\n").expect("failed to write base");
    repo.git(&["add", "delegate-fallback.txt"])
        .expect("add should succeed");
    repo.stage_all_and_commit("base commit")
        .expect("base commit should succeed");

    fs::write(
        repo.path().join("delegate-fallback.txt"),
        "base\nchanged without daemon\n",
    )
    .expect("failed to write updated file");

    // Shut down any stale daemon, then restart it manually.
    repo.shutdown_dedicated_daemon_for_test();

    // Manually restart the daemon (production auto-start is disabled in test builds)
    start_daemon_for_repo(&repo);

    let completion_baseline = repo.daemon_total_completion_count();
    repo.git_ai_with_env(
        &["checkpoint", "mock_ai", "delegate-fallback.txt"],
        &[("GIT_AI_DAEMON_CHECKPOINT_DELEGATE", "true")],
    )
    .expect("checkpoint should delegate to daemon and succeed");

    // Wait for the fire-and-forget checkpoint to complete
    repo.wait_for_next_daemon_checkpoint_completion(completion_baseline);

    let status = send_control_request(
        &daemon_control_socket_path(&repo),
        &ControlRequest::StatusFamily {
            repo_working_dir: repo_workdir_string(&repo),
        },
    )
    .expect("daemon status request should succeed");
    assert!(
        status.ok,
        "daemon should be running after delegated checkpoint; ok={}, error={:?}, data={:?}, socket={}, workdir={}",
        status.ok,
        status.error,
        status.data,
        daemon_control_socket_path(&repo).display(),
        repo_workdir_string(&repo)
    );
    let checkpoints = repo
        .current_working_logs()
        .read_all_checkpoints()
        .expect("checkpoints should be readable");
    assert!(
        checkpoints
            .iter()
            .any(|checkpoint| checkpoint.kind == CheckpointKind::AiAgent),
        "delegated checkpoint should write ai_agent checkpoint via daemon"
    );

    let _ = send_control_request(
        &daemon_control_socket_path(&repo),
        &ControlRequest::Shutdown,
    );
}

#[test]
#[serial]
fn checkpoint_fails_hard_when_daemon_startup_is_blocked() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::Dedicated);

    fs::write(repo.path().join("delegate-fallback-blocked.txt"), "base\n")
        .expect("failed to write base");
    repo.git(&["add", "delegate-fallback-blocked.txt"])
        .expect("add should succeed");
    repo.stage_all_and_commit("base commit")
        .expect("base commit should succeed");

    fs::write(
        repo.path().join("delegate-fallback-blocked.txt"),
        "base\nchanged while startup blocked\n",
    )
    .expect("failed to write updated file");

    repo.shutdown_dedicated_daemon_for_test();

    fs::create_dir_all(
        daemon_lock_path(&repo)
            .parent()
            .expect("daemon lock path should have a parent"),
    )
    .expect("failed to create daemon lock parent directory");
    let held_lock = DaemonLock::acquire(&daemon_lock_path(&repo))
        .expect("should acquire daemon lock before checkpoint invocation");

    let result = repo.git_ai(&["checkpoint", "mock_ai", "delegate-fallback-blocked.txt"]);
    assert!(
        result.is_ok(),
        "checkpoint should exit(0) when daemon is unavailable (never block agents)"
    );

    drop(held_lock);
}
