use super::*;
use git_ai::operations::daemon::{ControlRequest, send_control_request};
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;

struct BlockedPush {
    repo: TestRepo,
    _remote_dir: tempfile::TempDir,
    remote: std::path::PathBuf,
    gate: std::path::PathBuf,
    commit: String,
}

impl Drop for BlockedPush {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.gate);
    }
}

fn blocked_push(reject: bool) -> BlockedPush {
    let remote_dir = tempfile::tempdir().unwrap();
    let remote = remote_dir.path().join("remote.git");
    bare_repository(&remote);
    let repo = TestRepo::new_dedicated_daemon();
    let mut original = repo.filename("original.txt");
    original.set_contents(lines!["original AI".ai()]);
    let commit = repo
        .stage_all_and_commit("original attributed commit")
        .unwrap();
    original.assert_committed_lines(lines!["original AI".ai()]);
    repo.git_og(&["remote", "add", "origin", remote.to_str().unwrap()])
        .unwrap();
    let gate = remote.join("hooks/notes-push.block");
    let entered = remote.join("hooks/notes-push.entered");
    let hook = remote.join("hooks/pre-receive");
    let hook_body = r#"#!/bin/sh
while read old new ref; do
    if [ "$ref" = refs/notes/ai ]; then
        notes=1
        printf 'entered' > hooks/notes-push.entered
        attempts=0
        while [ -e hooks/notes-push.block ] && [ "$attempts" -lt 500 ]; do
            sleep 0.02
            attempts=$((attempts + 1))
        done
    fi
done
"#;
    fs::write(
        &hook,
        format!(
            "{hook_body}\nif [ \"$notes\" = 1 ]; then exit {}; fi\n",
            if reject { 1 } else { 0 }
        ),
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&gate, "").unwrap();

    repo.git_without_test_sync_for_test(PUSH_ARGS, &[]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !entered.exists() {
        assert!(
            Instant::now() < deadline,
            "notes push never reached the remote hook"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    BlockedPush {
        repo,
        _remote_dir: remote_dir,
        remote,
        gate,
        commit: commit.commit_sha,
    }
}

#[test]
fn same_family_checkpoint_completes_while_notes_delivery_is_blocked() {
    let blocked = blocked_push(false);
    let repo = &blocked.repo;
    let gate = &blocked.gate;
    fs::write(repo.path().join("pending.txt"), "later AI\n").unwrap();
    let mut checkpoint = repo
        .git_ai_command_without_pre_sync_for_test(&["checkpoint", "mock_ai", "pending.txt"], &[])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    let checkpoint_finished = loop {
        if checkpoint.try_wait().unwrap().is_some() {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let push_completed_before_release = repo
        .daemon_completion_entries()
        .iter()
        .any(|entry| entry.primary_command.as_deref() == Some("push"));
    let sync_socket = repo.daemon_control_socket_path();
    let worktree = repo.canonical_path().to_string_lossy().to_string();
    let sync = std::thread::spawn(move || {
        send_control_request(
            &sync_socket,
            &ControlRequest::SyncFamily {
                repo_working_dir: worktree,
            },
        )
    });
    let await_socket = repo.daemon_control_socket_path();
    let global_await = std::thread::spawn(move || {
        send_control_request(&await_socket, &ControlRequest::Await { timeout_secs: 10 })
    });
    std::thread::sleep(Duration::from_millis(100));
    let sync_finished_early = sync.is_finished();
    let await_finished_early = global_await.is_finished();
    fs::remove_file(gate).unwrap();
    let output = checkpoint.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let sync_response = sync.join().unwrap().unwrap();
    let await_response = global_await.join().unwrap().unwrap();
    assert!(
        !sync_finished_early,
        "family sync completed before notes delivery"
    );
    assert!(
        !await_finished_early,
        "global await completed before notes delivery"
    );
    assert!(sync_response.ok, "{sync_response:?}");
    assert!(await_response.ok, "{await_response:?}");
    assert_eq!(await_response.data.as_ref().unwrap()["done"], true);
    repo.sync_daemon_force();
    assert!(
        !push_completed_before_release,
        "push completion must remain fenced through delivery"
    );
    assert!(remote_note(&blocked.remote, &blocked.commit).is_some());
    repo.stage_all_and_commit("commit checkpoint accepted during notes push")
        .unwrap();
    repo.filename("original.txt")
        .assert_committed_lines(lines!["original AI".ai()]);
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["later AI".ai()]);
    assert!(
        checkpoint_finished,
        "a blocked notes transport held the same-family checkpoint acknowledgement"
    );
}

#[test]
fn shutdown_waits_for_accepted_notes_delivery_and_its_completion_record() {
    let mut blocked = blocked_push(false);
    let socket = blocked.repo.daemon_control_socket_path();
    let shutdown =
        std::thread::spawn(move || send_control_request(&socket, &ControlRequest::Shutdown));
    std::thread::sleep(Duration::from_millis(100));
    let finished_early = shutdown.is_finished();
    fs::remove_file(&blocked.gate).unwrap();
    let response = shutdown.join().unwrap().unwrap();
    assert!(!finished_early, "shutdown acknowledged undelivered notes");
    assert!(response.ok, "{response:?}");
    assert!(remote_note(&blocked.remote, &blocked.commit).is_some());
    let entries = blocked.repo.daemon_completion_entries();
    let pushes: Vec<_> = entries
        .iter()
        .filter(|entry| entry.primary_command.as_deref() == Some("push"))
        .collect();
    assert_eq!(pushes.len(), 1);
    assert_eq!(pushes[0].status, "ok");
    blocked.repo.restart_dedicated_daemon_for_test();
    blocked
        .repo
        .filename("original.txt")
        .assert_committed_lines(lines!["original AI".ai()]);
}

#[test]
fn late_notes_delivery_failure_remains_visible_after_a_newer_checkpoint_succeeds() {
    let blocked = blocked_push(true);
    let repo = &blocked.repo;
    fs::write(repo.path().join("pending.txt"), "later AI\n").unwrap();
    let checkpoint = repo
        .git_ai_command_without_pre_sync_for_test(&["checkpoint", "mock_ai", "pending.txt"], &[])
        .output()
        .unwrap();
    assert!(checkpoint.status.success(), "{checkpoint:?}");
    fs::remove_file(&blocked.gate).unwrap();
    repo.sync_daemon_force();
    let entries = repo.daemon_completion_entries();
    let pushes: Vec<_> = entries
        .iter()
        .filter(|entry| entry.primary_command.as_deref() == Some("push"))
        .collect();
    assert_eq!(pushes.len(), 1);
    assert_eq!(pushes[0].status, "error");
    assert!(
        pushes[0]
            .error
            .as_ref()
            .unwrap()
            .contains("pre-receive hook declined")
    );
    assert!(remote_note(&blocked.remote, &blocked.commit).is_none());
    let response = send_control_request(
        &repo.daemon_control_socket_path(),
        &ControlRequest::StatusFamily {
            repo_working_dir: repo.canonical_path().to_string_lossy().to_string(),
        },
    )
    .unwrap();
    assert!(response.ok, "{response:?}");
    assert!(
        response.data.unwrap()["last_error"]
            .as_str()
            .unwrap()
            .contains("pre-receive hook declined")
    );
    repo.stage_all_and_commit("commit after failed delivery")
        .unwrap();
    repo.filename("original.txt")
        .assert_committed_lines(lines!["original AI".ai()]);
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["later AI".ai()]);
}

#[test]
fn push_accepted_during_delivery_sends_the_later_commits_note_in_another_pass() {
    let blocked = blocked_push(false);
    let repo = &blocked.repo;
    fs::write(repo.path().join("later.txt"), "later AI\n").unwrap();
    let checkpoint = repo
        .git_ai_command_without_pre_sync_for_test(&["checkpoint", "mock_ai", "later.txt"], &[])
        .output()
        .unwrap();
    assert!(checkpoint.status.success(), "{checkpoint:?}");
    repo.git_without_test_sync_for_test(&["add", "later.txt"], &[])
        .unwrap();
    repo.git_without_test_sync_for_test(&["commit", "-m", "commit during notes delivery"], &[])
        .unwrap();
    let later = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !repo
        .daemon_completion_entries()
        .iter()
        .any(|entry| entry.commit_shas.contains(&later))
    {
        assert!(
            Instant::now() < deadline,
            "later commit waited for network delivery"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    repo.git_without_test_sync_for_test(PUSH_ARGS, &[]).unwrap();
    let barrier = repo
        .git_ai_command_without_pre_sync_for_test(&["checkpoint", "mock_ai", "later.txt"], &[])
        .output()
        .unwrap();
    assert!(barrier.status.success(), "{barrier:?}");
    assert!(
        !repo
            .daemon_completion_entries()
            .iter()
            .any(|entry| entry.primary_command.as_deref() == Some("push"))
    );
    fs::remove_file(&blocked.gate).unwrap();
    repo.sync_daemon_force();
    assert!(remote_note(&blocked.remote, &blocked.commit).is_some());
    assert!(remote_note(&blocked.remote, &later).is_some());
    let entries = repo.daemon_completion_entries();
    let pushes: Vec<_> = entries
        .iter()
        .filter(|entry| entry.primary_command.as_deref() == Some("push"))
        .collect();
    assert_eq!(pushes.len(), 2);
    assert!(pushes.iter().all(|entry| entry.status == "ok"));
    repo.filename("original.txt")
        .assert_committed_lines(lines!["original AI".ai()]);
    repo.filename("later.txt")
        .assert_committed_lines(lines!["later AI".ai()]);
}

#[test]
fn notes_worker_panic_records_failure_releases_fences_and_allows_a_later_push() {
    let temp = tempfile::tempdir().unwrap();
    let flag = temp.path().join("panic");
    let repo = TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_PANIC_IN_NOTES_PUSH_FLAG",
        flag.to_str().unwrap(),
    )]);
    let remote = temp.path().join("remote.git");
    bare_repository(&remote);
    let mut file = repo.filename("source.txt");
    file.set_contents(lines!["AI content".ai()]);
    let commit = repo.stage_all_and_commit("attributed content").unwrap();
    file.assert_committed_lines(lines!["AI content".ai()]);
    repo.git_og(&["remote", "add", "origin", remote.to_str().unwrap()])
        .unwrap();
    fs::write(&flag, "").unwrap();
    repo.git_without_test_sync_for_test(PUSH_ARGS, &[]).unwrap();
    repo.sync_daemon_force();
    let first = repo.daemon_completion_entries();
    let failures: Vec<_> = first
        .iter()
        .filter(|entry| entry.primary_command.as_deref() == Some("push"))
        .collect();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].status, "error");
    assert!(
        failures[0]
            .error
            .as_ref()
            .unwrap()
            .contains("test-induced notes push worker panic")
    );
    assert!(remote_note(&remote, &commit.commit_sha).is_none());
    fs::remove_file(&flag).unwrap();
    repo.git(PUSH_ARGS).unwrap();
    repo.sync_daemon_force();
    let entries = repo.daemon_completion_entries();
    assert!(
        remote_note(&remote, &commit.commit_sha).is_some(),
        "later push completion: {entries:?}"
    );
    let pushes: Vec<_> = entries
        .iter()
        .filter(|entry| entry.primary_command.as_deref() == Some("push"))
        .collect();
    assert_eq!(pushes.len(), 2);
    assert_eq!(pushes[1].status, "ok");
    file.assert_committed_lines(lines!["AI content".ai()]);
}

#[test]
fn oversized_completion_metadata_reports_failure_without_retaining_a_delivery() {
    let temp = tempfile::tempdir().unwrap();
    let remote = temp.path().join("remote.git");
    bare_repository(&remote);
    let repo = TestRepo::new_dedicated_daemon();
    let mut file = repo.filename("source.txt");
    file.set_contents(lines!["AI content".ai()]);
    let commit = repo.stage_all_and_commit("attributed content").unwrap();
    file.assert_committed_lines(lines!["AI content".ai()]);
    repo.git_og(&["remote", "add", "origin", remote.to_str().unwrap()])
        .unwrap();
    let session = format!(
        "{}={}",
        git_ai::operations::daemon::test_sync::TEST_SYNC_SESSION_CONFIG_KEY,
        "s".repeat(64 * 1024)
    );
    repo.git_without_test_sync_for_test(
        &["-c", &session, "push", "origin", "HEAD:refs/heads/main"],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    let entries = repo.daemon_completion_entries();
    let pushes: Vec<_> = entries
        .iter()
        .filter(|entry| entry.primary_command.as_deref() == Some("push"))
        .collect();
    assert_eq!(pushes.len(), 1);
    assert_eq!(pushes[0].status, "error");
    assert!(
        pushes[0]
            .error
            .as_ref()
            .unwrap()
            .contains("completion metadata limit exceeded")
    );
    assert!(remote_note(&remote, &commit.commit_sha).is_none());
    repo.git(PUSH_ARGS).unwrap();
    repo.sync_daemon_force();
    assert!(remote_note(&remote, &commit.commit_sha).is_some());
    file.assert_committed_lines(lines!["AI content".ai()]);
}
