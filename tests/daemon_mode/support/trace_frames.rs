use super::*;

pub(super) fn send_trace_frames(trace_socket_path: &Path, payloads: &[Value]) {
    let mut stream =
        open_local_socket_stream_with_timeout(trace_socket_path, DAEMON_TEST_PROBE_TIMEOUT)
            .expect("failed to connect to trace socket");
    for payload in payloads {
        let raw = serde_json::to_string(payload).expect("failed to serialize trace payload");
        stream
            .write_all(raw.as_bytes())
            .expect("failed to write trace payload");
        stream
            .write_all(b"\n")
            .expect("failed to write trace newline");
    }
    stream.flush().expect("failed to flush trace payloads");
}

pub(super) fn trace_atexit_frame(sid: &str, code: i32, time_ns: u64) -> Value {
    json!({
        "event": "atexit",
        "sid": sid,
        "code": code,
        "time_ns": time_ns,
    })
}

pub(super) struct TraceCommandFrames {
    pub(super) sid: String,
    pub(super) argv: Vec<String>,
    pub(super) worktree: String,
    pub(super) repo: String,
    pub(super) start_time_ns: u64,
    pub(super) exit_code: i32,
}

impl TraceCommandFrames {
    pub(super) fn new(
        sid: &str,
        argv: &[&str],
        worktree: &str,
        repo: &str,
        start_time_ns: u64,
    ) -> Self {
        Self {
            sid: sid.to_string(),
            argv: argv.iter().map(|arg| (*arg).to_string()).collect(),
            worktree: worktree.to_string(),
            repo: repo.to_string(),
            start_time_ns,
            exit_code: 0,
        }
    }

    pub(super) fn with_exit_code(mut self, exit_code: i32) -> Self {
        self.exit_code = exit_code;
        self
    }

    pub(super) fn into_frames(self) -> Vec<Value> {
        let repository_time_ns = self.start_time_ns + 1;
        let exit_time_ns = self.start_time_ns + 100;
        let atexit_time_ns = self.start_time_ns + 101;

        vec![
            json!({
                "event": "start",
                "sid": &self.sid,
                "argv": &self.argv,
                "time_ns": self.start_time_ns,
            }),
            json!({
                "event": "def_repo",
                "sid": &self.sid,
                "worktree": &self.worktree,
                "repo": &self.repo,
                "time_ns": repository_time_ns,
            }),
            json!({
                "event": "exit",
                "sid": &self.sid,
                "code": self.exit_code,
                "time_ns": exit_time_ns,
            }),
            trace_atexit_frame(&self.sid, self.exit_code, atexit_time_ns),
        ]
    }
}

#[test]
fn trace_command_frames_emit_a_complete_deterministic_lifecycle() {
    let frames = TraceCommandFrames::new(
        "trace-command-frames",
        &["git", "status"],
        "/repo",
        "/repo/.git",
        1_000,
    )
    .with_exit_code(1)
    .into_frames();

    assert_eq!(
        frames,
        vec![
            json!({
                "event": "start",
                "sid": "trace-command-frames",
                "argv": ["git", "status"],
                "time_ns": 1_000u64,
            }),
            json!({
                "event": "def_repo",
                "sid": "trace-command-frames",
                "worktree": "/repo",
                "repo": "/repo/.git",
                "time_ns": 1_001u64,
            }),
            json!({
                "event": "exit",
                "sid": "trace-command-frames",
                "code": 1,
                "time_ns": 1_100u64,
            }),
            json!({
                "event": "atexit",
                "sid": "trace-command-frames",
                "code": 1,
                "time_ns": 1_101u64,
            }),
        ],
    );
}

#[cfg(not(windows))]
pub(super) fn write_trace_frames_to_stream(stream: &mut impl Write, payloads: &[Value]) {
    for payload in payloads {
        let raw = serde_json::to_string(payload).expect("failed to serialize trace payload");
        stream
            .write_all(raw.as_bytes())
            .expect("failed to write trace payload");
        stream
            .write_all(b"\n")
            .expect("failed to write trace newline");
    }
    stream.flush().expect("failed to flush trace payloads");
}

pub(super) fn git_trace_env(trace_socket_path: &Path) -> [(&'static str, String); 2] {
    [
        (
            "GIT_TRACE2_EVENT",
            DaemonConfig::trace2_event_target_for_path(trace_socket_path),
        ),
        ("GIT_TRACE2_EVENT_NESTING", "0".to_string()),
    ]
}

pub(super) fn traced_git_with_env(
    repo: &TestRepo,
    args: &[&str],
    envs: &[(&str, &str)],
    expected_top_level_completions: &mut u64,
) -> Result<String, String> {
    *expected_top_level_completions += 1;
    repo.git_og_with_env(args, envs)
}

pub(super) fn wait_for_expected_top_level_completions(
    repo: &TestRepo,
    baseline: u64,
    expected_top_level_completions: u64,
) {
    repo.wait_for_daemon_total_completion_count(
        baseline,
        baseline.saturating_add(expected_top_level_completions),
    );
}

pub(super) fn completion_entries_for_command(
    repo: &TestRepo,
    command: &str,
) -> Vec<DaemonTestCompletionLogEntry> {
    repo.daemon_completion_entries()
        .into_iter()
        .filter(|entry| entry.primary_command.as_deref() == Some(command))
        .collect()
}
