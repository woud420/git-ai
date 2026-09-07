//! Cross-family drain concurrency: a slow side effect in one repository
//! family must not stall attribution for other families on the same daemon.

use super::*;

fn family_b_git_ai(repo: &TestRepo, workdir: &Path, args: &[&str]) {
    let mut command = Command::new(get_binary_path());
    command.args(args).current_dir(workdir);
    configure_test_home_env(&mut command, repo.test_home_path());
    command.env("GIT_AI_TEST_DB_PATH", repo.test_db_path());
    command.env("GITAI_TEST_DB_PATH", repo.test_db_path());
    if let Some(patch) = repo.config_patch_json() {
        command.env("GIT_AI_TEST_CONFIG_PATCH", patch);
    }
    command.env("GIT_AI_DAEMON_HOME", repo.daemon_home_path());
    command.env(
        "GIT_AI_DAEMON_CONTROL_SOCKET",
        repo.daemon_control_socket_path(),
    );
    command.env(
        "GIT_AI_DAEMON_TRACE_SOCKET",
        repo.daemon_trace_socket_path(),
    );
    command.env("GIT_AI_DAEMON_CHECKPOINT_DELEGATE", "true");
    let output = command.output().expect("failed to run git-ai for family B");
    assert!(
        output.status.success(),
        "family B git-ai {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn family_b_note_exists(repo: &TestRepo, workdir: &Path) -> bool {
    RawGitCommand::in_working_dir(workdir, &["notes", "--ref=ai", "show", "HEAD"])
        .configure(|command| configure_test_home_env(command, repo.test_home_path()))
        .output()
        .expect("failed to probe family B note")
        .status
        .success()
}

/// Family B must finish attribution while family A is explicitly held in
/// its rebase side effect. A fixed sleep cannot establish this ordering on
/// a busy runner because B's setup can outlast the sleep.
#[test]
fn slow_family_side_effect_does_not_stall_other_families() {
    let gate_dir = tempfile::tempdir().expect("create side-effect gate directory");
    let gate = gate_dir.path().join("rebase-gate");
    fs::write(&gate, "").expect("create side-effect gate");
    let gate_spec = format!("rebase={}", gate.display());
    let repo = TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND",
        gate_spec.as_str(),
    )]);

    fs::write(repo.path().join("a-base.txt"), "base\n").expect("failed to write base");
    repo.git(&["add", "a-base.txt"]).expect("stage base");
    repo.git(&["commit", "-m", "base"]).expect("commit base");
    repo.filename("a-base.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    fs::write(repo.path().join("a-second.txt"), "second\n").expect("failed to write second");
    repo.git(&["add", "a-second.txt"]).expect("stage second");
    repo.git(&["commit", "-m", "second"])
        .expect("commit second");
    repo.filename("a-base.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    repo.filename("a-second.txt")
        .assert_committed_lines(lines!["second".unattributed_human()]);

    repo.git(&["rebase", "--force-rebase", "HEAD~1"])
        .expect("rebase should succeed");
    let entered_deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            std::time::Instant::now() < entered_deadline,
            "family A never entered its rebase side-effect gate"
        );
        thread::sleep(Duration::from_millis(25));
    }

    // Family B: a fresh repository under the allowed temp root, driven
    // through the same daemon's sockets.
    let other = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let family_b = other.path().to_path_buf();
    let harness = WorkdirRaceHarness::new(&repo, repo.daemon_trace_socket_path());
    fs::write(family_b.join("b.txt"), "ai line for family b\n")
        .expect("failed to write family B file");
    family_b_git_ai(&repo, &family_b, &["checkpoint", "mock_ai", "b.txt"]);
    harness.run_traced_git(&family_b, &["add", "b.txt"]);
    let committed_at = std::time::Instant::now();
    harness.run_traced_git(&family_b, &["commit", "-m", "family b commit"]);

    let deadline = committed_at + Duration::from_secs(20);
    while !family_b_note_exists(&repo, &family_b) {
        assert!(
            std::time::Instant::now() < deadline,
            "family B's authorship note did not land while family A's delayed \
             rebase side effect was still running"
        );
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        completion_entries_for_command(&repo, "rebase").len(),
        0,
        "family A's delayed rebase side effect should still be in flight when \
         family B's note lands"
    );
    other
        .filename("b.txt")
        .assert_committed_lines(lines!["ai line for family b".ai()]);

    fs::remove_file(&gate).expect("release family A's rebase side effect");
    let rebase_deadline = std::time::Instant::now() + Duration::from_secs(20);
    while completion_entries_for_command(&repo, "rebase").is_empty() {
        assert!(
            std::time::Instant::now() < rebase_deadline,
            "family A's rebase side effect never completed"
        );
        thread::sleep(Duration::from_millis(100));
    }
    repo.filename("a-base.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    repo.filename("a-second.txt")
        .assert_committed_lines(lines!["second".unattributed_human()]);
}
