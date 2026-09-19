use super::*;
use std::os::unix::process::CommandExt;
use std::time::Instant;

struct WaitingCommit {
    child: Option<Child>,
    release: PathBuf,
}

impl WaitingCommit {
    fn release(mut self) -> std::process::ExitStatus {
        fs::write(&self.release, "").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                self.child = None;
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "commit did not exit after its editor returned"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for WaitingCommit {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, "");
        if let Some(child) = self.child.as_mut() {
            // The editor shares this owned group; failed assertions must not leave it running.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
        }
    }
}

#[test]
fn real_commit_waiting_for_editor_preserves_later_commit_attribution() {
    let repo = TestRepo::new_dedicated_daemon();
    prepare_ai_edit(&repo);
    let mut file = repo.filename("editor-wait.txt");

    let script = repo.test_home_path().join("blocking-editor.sh");
    let started = repo.test_home_path().join("editor-started");
    let release = repo.test_home_path().join("editor-release");
    fs::write(&script, "touch \"$GIT_AI_TEST_EDITOR_STARTED\"\nwhile [ ! -e \"$GIT_AI_TEST_EDITOR_RELEASE\" ]; do sleep 0.05; done\nexit 1\n").unwrap();
    let child = RawGitCommand::in_working_dir(repo.path(), &["commit"])
        .configure(|command| {
            configure_test_home_env(command, repo.test_home_path());
            command
                .process_group(0)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
        })
        .env("GIT_EDITOR", "sh \"$GIT_AI_TEST_EDITOR_SCRIPT\"")
        .env("GIT_AI_TEST_EDITOR_SCRIPT", &script)
        .env("GIT_AI_TEST_EDITOR_STARTED", &started)
        .env("GIT_AI_TEST_EDITOR_RELEASE", &release)
        .env(
            "GIT_TRACE2_EVENT",
            DaemonConfig::trace2_event_target_for_path(&daemon_trace_socket_path(&repo)),
        )
        .env("GIT_TRACE2_EVENT_NESTING", "0")
        .spawn()
        .unwrap();
    let waiting = WaitingCommit {
        child: Some(child),
        release,
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !started.exists() {
        assert!(Instant::now() < deadline, "commit never opened its editor");
        thread::sleep(Duration::from_millis(10));
    }
    repo.git_without_test_sync_for_test(&["commit", "-m", "later commit"], &[])
        .unwrap();
    let sync = send_control_request_with_timeout(
        &daemon_control_socket_path(&repo),
        &ControlRequest::SyncFamily {
            repo_working_dir: repo_workdir_string(&repo),
        },
        Duration::from_secs(2),
    );
    assert!(
        !waiting.release().success(),
        "the first editor deliberately cancels its commit"
    );
    let sync = sync.expect("the later commit must sync while the first editor remains open");
    assert!(sync.ok, "{sync:?}");
    file.assert_committed_lines(lines!["Human base".human(), "AI line".ai()]);
}

fn prepare_ai_edit(repo: &TestRepo) {
    let mut file = repo.filename("editor-wait.txt");
    let path = repo.path().join("editor-wait.txt");
    fs::write(&path, "Human base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "editor-wait.txt"])
        .unwrap();
    repo.stage_all_and_commit("base").unwrap();
    file.assert_committed_lines(lines!["Human base".human()]);
    repo.git_ai(&["checkpoint", "human", "editor-wait.txt"])
        .unwrap();
    fs::write(&path, "Human base\nAI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "editor-wait.txt"])
        .unwrap();
    repo.git(&["add", "editor-wait.txt"]).unwrap();
}

#[test]
fn rebase_waiting_for_editor_remains_an_ordering_barrier() {
    assert_editor_barrier("rebase", None);
}

#[test]
fn commit_with_prior_nested_mutation_keeps_its_editor_barrier() {
    assert_editor_barrier("commit", Some(false));
}

#[test]
fn nested_mutation_during_commit_editor_restores_the_barrier() {
    assert_editor_barrier("commit", Some(true));
}

fn assert_editor_barrier(command: &str, mutation_after_editor: Option<bool>) {
    let repo = TestRepo::new_dedicated_daemon();
    prepare_ai_edit(&repo);
    let socket = daemon_trace_socket_path(&repo);
    let mut trace =
        open_local_socket_stream_with_timeout(&socket, DAEMON_TEST_PROBE_TIMEOUT).unwrap();
    let mut frames = vec![
        json!({"event":"start","sid":"editor-barrier","argv":["git",command],"time_ns":1000u64}),
        json!({"event":"def_repo","sid":"editor-barrier","worktree":repo_workdir_string(&repo),"repo":repo.path().join(".git"),"time_ns":1001u64}),
    ];
    let editor = json!({"event":"child_start","sid":"editor-barrier","child_class":"editor","child_id":7,"time_ns":1010u64});
    let mutation = json!({"event":"start","sid":"editor-barrier/nested","argv":["git","update-ref","refs/heads/main","HEAD"],"time_ns":1005u64});
    if mutation_after_editor == Some(false) {
        frames.push(mutation.clone());
    }
    frames.push(editor);
    if mutation_after_editor == Some(true) {
        frames.push(mutation);
    }
    if mutation_after_editor.is_some() {
        frames.push(trace_atexit_frame("editor-barrier/nested", 0, 1012));
    }
    write_trace_frames_to_stream(&mut trace, &frames);
    repo.git_without_test_sync_for_test(&["commit", "-m", "commit behind barrier"], &[])
        .unwrap();
    let blocked = send_control_request_with_timeout(
        &daemon_control_socket_path(&repo),
        &ControlRequest::SyncFamily {
            repo_working_dir: repo_workdir_string(&repo),
        },
        Duration::from_millis(250),
    );
    let timeout = blocked.expect_err("the active mutation must still block family synchronization");
    assert!(timeout.to_string().contains("timed out"), "{timeout}");
    write_trace_frames_to_stream(
        &mut trace,
        &[
            json!({"event":"child_exit","sid":"editor-barrier","child_id":7,"code":1,"time_ns":2000u64}),
            trace_atexit_frame("editor-barrier", 1, 2001),
        ],
    );
    drop(trace);
    repo.sync_daemon();
    repo.filename("editor-wait.txt")
        .assert_committed_lines(lines!["Human base".human(), "AI line".ai()]);
}
