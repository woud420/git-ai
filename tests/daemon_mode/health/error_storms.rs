use super::*;

fn checkpoint(repo: &TestRepo) -> std::process::Output {
    repo.git_ai_command_without_pre_sync_for_test(&["checkpoint", "mock_ai", "storm.txt"], &[])
        .output()
        .expect("checkpoint process must start")
}

fn checkpoint_error_count(repo: &TestRepo) -> usize {
    repo.daemon_diagnostics()
        .1
        .lines()
        .filter(|line| line.contains("ERROR") && line.contains("checkpoint side effect failed"))
        .count()
}

fn wait_for_failures(repo: &TestRepo, expected: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = bg_status(repo, repo.path());
        if status["daemon"]["side_effect_errors_total"]
            .as_u64()
            .unwrap()
            >= expected
            && status["daemon"]["checkpoints_outstanding"] == json!(0)
        {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "checkpoint failure not recorded: {status}; {}",
            repo.daemon_diagnostics().1
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn repeated_checkpoint_errors_remain_in_health_and_rearm_logging_after_recovery() {
    let repo = TestRepo::new_with_daemon_env(&[("RUST_LOG", "info")]);
    fs::write(repo.path().join("storm.txt"), "untracked seed\n").unwrap();
    repo.stage_all_and_commit("Seed before daemon failure")
        .unwrap();
    repo.filename("storm.txt")
        .assert_committed_lines(lines!["untracked seed".unattributed_human()]);

    let working_logs = repo.path().join(".git/ai/working_logs");
    fs::create_dir_all(&working_logs).unwrap();
    let saved_logs = repo.path().join(".git/ai/saved-working-logs");
    fs::write(repo.path().join("storm.txt"), "recovered AI\n").unwrap();
    fs::rename(&working_logs, &saved_logs).unwrap();
    fs::write(&working_logs, "blocked directory").unwrap();
    for expected in 1..=6 {
        let output = checkpoint(&repo);
        assert!(output.status.success(), "{output:?}");
        wait_for_failures(&repo, expected);
    }
    fs::remove_file(&working_logs).unwrap();
    fs::rename(&saved_logs, &working_logs).unwrap();
    let status = bg_status(&repo, repo.path());
    assert!(
        status["data"]["last_error"].is_string(),
        "{status}; {}",
        repo.daemon_diagnostics().1
    );
    assert_eq!(
        status["daemon"]["side_effect_errors_total"],
        json!(6),
        "{status}"
    );
    assert_eq!(
        checkpoint_error_count(&repo),
        3,
        "repeated failures should retain the first three error logs"
    );
    assert_eq!(
        repo.daemon_diagnostics()
            .1
            .lines()
            .filter(|line| line.contains("WARN")
                && line.contains("repeated daemon failures suppressed"))
            .count(),
        1,
        "suppressed failures should produce a single summary in this interval"
    );

    let recovered = checkpoint(&repo);
    assert!(recovered.status.success(), "{recovered:?}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while bg_status(&repo, repo.path())["daemon"]["checkpoints_outstanding"] != json!(0) {
        assert!(
            Instant::now() < deadline,
            "recovered checkpoint did not finish"
        );
        thread::sleep(Duration::from_millis(20));
    }
    fs::rename(&working_logs, &saved_logs).unwrap();
    fs::write(&working_logs, "blocked directory").unwrap();
    let output = checkpoint(&repo);
    assert!(output.status.success(), "{output:?}");
    wait_for_failures(&repo, 7);
    assert_eq!(
        checkpoint_error_count(&repo),
        4,
        "success must rearm error-level logging"
    );

    fs::remove_file(&working_logs).unwrap();
    fs::rename(&saved_logs, &working_logs).unwrap();
    repo.stage_all_and_commit("Commit after daemon recovery")
        .unwrap();
    repo.filename("storm.txt")
        .assert_committed_lines(lines!["recovered AI".ai()]);
}
