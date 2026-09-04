//! Cross-family drain concurrency: a slow side effect in one repository
//! family must not stall attribution for other families on the same daemon.

use super::*;
#[cfg(not(windows))]
use crate::repos::test_file::TestFile;

fn family_git_ai_command(repo: &TestRepo, workdir: &Path, args: &[&str]) -> Command {
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
    command
}

fn family_b_git_ai(repo: &TestRepo, workdir: &Path, args: &[&str]) {
    let mut command = family_git_ai_command(repo, workdir, args);
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

#[cfg(not(windows))]
fn write_am_patch(repo: &TestRepo) {
    let patch = "\
From 1111111111111111111111111111111111111111 Mon Sep 17 00:00:00 2001
From: Repro <repro@example.com>
Date: Mon, 1 Sep 2026 00:00:00 +0000
Subject: [PATCH] add am file

---
 am.txt | 1 +
 1 file changed, 1 insertion(+)
 create mode 100644 am.txt

diff --git a/am.txt b/am.txt
new file mode 100644
index 0000000..7898192
--- /dev/null
+++ b/am.txt
@@ -0,0 +1 @@
+a
--\x20
2.39.0
";
    fs::write(repo.path().join("am-patch.mbox"), patch).expect("write git am patch");
}

#[cfg(not(windows))]
fn initialize_family_repo(repo: &TestRepo, workdir: &Path) {
    fs::create_dir_all(workdir).expect("create family repository");
    let init = RawGitCommand::in_working_dir(workdir, &["init"])
        .configure(|command| configure_test_home_env(command, repo.test_home_path()))
        .output()
        .expect("git init should run");
    assert!(init.status.success(), "git init failed: {init:?}");
    for (key, value) in [("user.email", "b@example.com"), ("user.name", "Family B")] {
        let config = RawGitCommand::in_working_dir(workdir, &["config", key, value])
            .configure(|command| configure_test_home_env(command, repo.test_home_path()))
            .output()
            .expect("git config should run");
        assert!(config.status.success(), "git config failed: {config:?}");
    }
}

#[test]
#[cfg(not(windows))]
fn delayed_nonsequencer_side_effect_does_not_stall_later_ingress() {
    const AM_DELAY_MS: u64 = 8_000;
    let delay = format!("am={AM_DELAY_MS}");
    let marker_dir = tempfile::tempdir().expect("create delay marker directory");
    let marker_path = marker_dir.path().join("am-delay-started");
    let marker = marker_path.to_string_lossy().to_string();
    let repo = TestRepo::new_with_daemon_env(&[
        (
            "GIT_AI_TEST_DELAY_SIDE_EFFECT_MS_FOR_COMMAND",
            delay.as_str(),
        ),
        (
            "GIT_AI_TEST_SIDE_EFFECT_DELAY_STARTED_PATH",
            marker.as_str(),
        ),
    ]);

    fs::write(repo.path().join("base.txt"), "base\n").expect("write base file");
    repo.git(&["add", "base.txt"]).expect("stage base file");
    repo.git(&["commit", "-m", "base"])
        .expect("commit base file");
    repo.filename("base.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    write_am_patch(&repo);
    repo.git_without_test_sync_for_test(&["am", "am-patch.mbox"], &[])
        .expect("git am should succeed");

    let marker_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !marker_path.exists() {
        assert!(
            std::time::Instant::now() < marker_deadline,
            "git am never entered its delayed side-effect pass"
        );
        thread::sleep(Duration::from_millis(25));
    }
    assert!(
        completion_entries_for_command(&repo, "am").is_empty(),
        "git am side-effect delay must still be active before the concurrency assertions"
    );

    fs::write(repo.path().join("same-family.txt"), "same family\n")
        .expect("write same-family checkpoint file");
    let mut same_family_command = family_git_ai_command(
        &repo,
        repo.path(),
        &["checkpoint", "mock_ai", "same-family.txt"],
    );
    let same_family_checkpoint = thread::spawn(move || same_family_command.output());
    thread::sleep(Duration::from_millis(250));
    assert!(
        !same_family_checkpoint.is_finished(),
        "same-family acknowledgement must wait for the earlier git am side effect"
    );
    let sync_socket = repo.daemon_control_socket_path();
    let sync_worktree = repo.path().to_string_lossy().to_string();
    let sync = thread::spawn(move || {
        send_control_request(
            &sync_socket,
            &ControlRequest::SyncFamily {
                repo_working_dir: sync_worktree,
            },
        )
    });
    thread::sleep(Duration::from_millis(250));
    assert!(
        !sync.is_finished(),
        "sync.family must wait for the detached git am side effect"
    );

    let other = tempfile::tempdir().expect("create independent family directory");
    let family_b = other.path().join("family-b");
    initialize_family_repo(&repo, &family_b);
    fs::write(family_b.join("b.txt"), "ai line for family b\n")
        .expect("write independent family file");

    let independent_started = std::time::Instant::now();
    family_b_git_ai(&repo, &family_b, &["checkpoint", "mock_ai", "b.txt"]);
    let harness = WorkdirRaceHarness::new(&repo, repo.daemon_trace_socket_path());
    harness.run_traced_git(&family_b, &["add", "b.txt"]);
    harness.run_traced_git(&family_b, &["commit", "-m", "family b commit"]);
    while !family_b_note_exists(&repo, &family_b) {
        assert!(
            independent_started.elapsed() < Duration::from_secs(4),
            "independent trace and checkpoint work stalled behind git am"
        );
        thread::sleep(Duration::from_millis(25));
    }
    let family_b_blame = family_git_ai_command(&repo, &family_b, &["blame", "b.txt"])
        .output()
        .expect("run family B blame");
    assert!(
        family_b_blame.status.success(),
        "family B blame failed: {}",
        String::from_utf8_lossy(&family_b_blame.stderr)
    );
    TestFile::assert_committed_blame_output(
        &String::from_utf8_lossy(&family_b_blame.stdout),
        lines!["ai line for family b".ai()],
    );
    assert!(
        independent_started.elapsed() < Duration::from_secs(4),
        "independent trace and checkpoint work completed only after the delayed effect"
    );
    assert!(
        completion_entries_for_command(&repo, "am").is_empty(),
        "independent family work must finish before the delayed git am side effect"
    );
    assert!(
        !same_family_checkpoint.is_finished(),
        "same-family acknowledgement must remain ordered behind git am"
    );
    assert!(
        !sync.is_finished(),
        "sync.family must remain fenced while git am is still delayed"
    );

    let am_deadline = std::time::Instant::now() + Duration::from_secs(20);
    while completion_entries_for_command(&repo, "am").is_empty() {
        assert!(
            std::time::Instant::now() < am_deadline,
            "delayed git am side effect never completed"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let same_family_output = same_family_checkpoint
        .join()
        .expect("same-family checkpoint thread should not panic")
        .expect("same-family checkpoint command should run");
    assert!(
        same_family_output.status.success(),
        "same-family checkpoint failed: {}",
        String::from_utf8_lossy(&same_family_output.stderr)
    );
    let sync_response = sync
        .join()
        .expect("sync.family thread should not panic")
        .expect("sync.family request should succeed");
    assert!(sync_response.ok, "sync.family failed: {sync_response:?}");
    repo.filename("am.txt")
        .assert_committed_lines(lines!["a".unattributed_human()]);
}

#[test]
#[cfg(not(windows))]
fn graceful_shutdown_waits_for_detached_nonsequencer_side_effect() {
    let marker_dir = tempfile::tempdir().expect("create delay marker directory");
    let marker_path = marker_dir.path().join("am-shutdown-delay-started");
    let marker = marker_path.to_string_lossy().to_string();
    let mut repo = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_TEST_DELAY_SIDE_EFFECT_MS_FOR_COMMAND", "am=1500"),
        (
            "GIT_AI_TEST_SIDE_EFFECT_DELAY_STARTED_PATH",
            marker.as_str(),
        ),
    ]);

    fs::write(repo.path().join("base.txt"), "base\n").expect("write base file");
    repo.git(&["add", "base.txt"]).expect("stage base file");
    repo.git(&["commit", "-m", "base"])
        .expect("commit base file");
    repo.filename("base.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    write_am_patch(&repo);
    repo.git_without_test_sync_for_test(&["am", "am-patch.mbox"], &[])
        .expect("git am should succeed");

    let marker_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !marker_path.exists() {
        assert!(
            std::time::Instant::now() < marker_deadline,
            "git am never entered its delayed side-effect pass"
        );
        thread::sleep(Duration::from_millis(25));
    }

    let socket = repo.daemon_control_socket_path();
    let (response_tx, response_rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = response_tx.send(send_control_request(&socket, &ControlRequest::Shutdown));
    });
    assert!(
        matches!(
            response_rx.recv_timeout(Duration::from_millis(250)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "shutdown responded before the accepted git am side effect completed"
    );
    let response = response_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("shutdown response timed out")
        .expect("shutdown request failed");
    assert!(response.ok, "shutdown response should be ok: {response:?}");
    assert_eq!(
        completion_entries_for_command(&repo, "am").len(),
        1,
        "shutdown must persist completion before acknowledging"
    );
    repo.restart_dedicated_daemon_for_test();
    repo.filename("am.txt")
        .assert_committed_lines(lines!["a".unattributed_human()]);
}

#[test]
#[cfg(not(windows))]
fn detached_nonsequencer_panic_records_failure_and_releases_fences() {
    let flag_dir = tempfile::tempdir().expect("create panic flag directory");
    let flag_path = flag_dir.path().join("panic-side-effect");
    let flag = flag_path.to_string_lossy().to_string();
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_PANIC_IN_SIDE_EFFECT_FLAG", flag.as_str())]);

    fs::write(repo.path().join("base.txt"), "base\n").expect("write base file");
    repo.git(&["add", "base.txt"]).expect("stage base file");
    repo.git(&["commit", "-m", "base"])
        .expect("commit base file");
    repo.filename("base.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    write_am_patch(&repo);
    fs::write(&flag_path, "panic").expect("arm side-effect panic");
    repo.git_without_test_sync_for_test(&["am", "am-patch.mbox"], &[])
        .expect("git am should succeed before daemon processing");

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let completion = loop {
        if let Some(completion) = completion_entries_for_command(&repo, "am")
            .into_iter()
            .next()
        {
            break completion;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "panicked git am side effect never produced a completion record"
        );
        thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(completion.status, "error");
    assert!(
        completion
            .error
            .as_deref()
            .is_some_and(|error| error.contains("test-induced panic")),
        "panic completion should preserve its diagnostic: {completion:?}"
    );

    fs::remove_file(flag_path).expect("disarm side-effect panic");
    repo.sync_daemon_force();
    let response = send_control_request(&repo.daemon_control_socket_path(), &ControlRequest::Ping)
        .expect("daemon should remain responsive after the detached panic");
    assert!(response.ok, "daemon ping should succeed: {response:?}");
    repo.filename("am.txt")
        .assert_committed_lines(lines!["a".unattributed_human()]);
}

/// Family A runs a rebase whose side effect is delayed by 6s (test hook).
/// Family B then checkpoints and commits on the same daemon. B's authorship
/// note must land well before A's rebase side effect finishes — with
/// serialized drains (the old behavior) the single ingest worker sleeps
/// inside A's drain, so B's checkpoint fence and post-commit authorship
/// cannot complete before the 6s delay elapses.
#[test]
fn slow_family_side_effect_does_not_stall_other_families() {
    const REBASE_DELAY_MS: u64 = 6_000;
    let delay = format!("rebase={REBASE_DELAY_MS}");
    let repo = TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_DELAY_SIDE_EFFECT_MS_FOR_COMMAND",
        delay.as_str(),
    )]);

    // Family A: two commits, then rewrite the last one so the daemon sees a
    // top-level rebase whose side effect sleeps for REBASE_DELAY_MS.
    fs::write(repo.path().join("a-base.txt"), "base\n").expect("failed to write base");
    repo.git(&["add", "a-base.txt"]).expect("stage base");
    repo.git(&["commit", "-m", "base"]).expect("commit base");
    fs::write(repo.path().join("a-second.txt"), "second\n").expect("failed to write second");
    repo.git(&["add", "a-second.txt"]).expect("stage second");
    repo.git(&["commit", "-m", "second"])
        .expect("commit second");

    repo.git(&["rebase", "--force-rebase", "HEAD~1"])
        .expect("rebase should succeed");

    // Family B: a fresh repository under the allowed temp root, driven
    // through the same daemon's sockets.
    let other = tempfile::tempdir().expect("failed to create family B dir");
    let family_b = other.path().join("family-b");
    fs::create_dir_all(&family_b).expect("failed to create family B repo dir");
    let harness = WorkdirRaceHarness::new(&repo, repo.daemon_trace_socket_path());
    RawGitCommand::in_working_dir(&family_b, &["init"])
        .configure(|command| configure_test_home_env(command, repo.test_home_path()))
        .output()
        .expect("git init should run");
    for (key, value) in [("user.email", "b@example.com"), ("user.name", "Family B")] {
        RawGitCommand::in_working_dir(&family_b, &["config", key, value])
            .configure(|command| configure_test_home_env(command, repo.test_home_path()))
            .output()
            .expect("git config should run");
    }
    fs::write(family_b.join("b.txt"), "ai line for family b\n")
        .expect("failed to write family B file");
    family_b_git_ai(&repo, &family_b, &["checkpoint", "mock_ai", "b.txt"]);
    harness.run_traced_git(&family_b, &["add", "b.txt"]);
    let committed_at = std::time::Instant::now();
    harness.run_traced_git(&family_b, &["commit", "-m", "family b commit"]);

    // B's post-commit authorship must land while A's rebase side effect is
    // still sleeping.
    let deadline = committed_at + Duration::from_millis(4_000);
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

    // Let family A finish so the daemon shuts down with no in-flight work.
    let rebase_deadline = std::time::Instant::now() + Duration::from_secs(20);
    while completion_entries_for_command(&repo, "rebase").is_empty() {
        assert!(
            std::time::Instant::now() < rebase_deadline,
            "family A's rebase side effect never completed"
        );
        thread::sleep(Duration::from_millis(100));
    }
}
