use super::environment::{
    configure_test_daemon_env, configure_test_home_env, daemon_control_socket_path,
    daemon_trace_socket_path, repo_workdir_string,
};
use super::*;

/// Outcome of a failed `DaemonGuard` readiness wait: a transient Windows loader
/// hiccup (respawn) versus a genuine failure (fail loudly).
pub(super) enum DaemonReadyOutcome {
    LoaderInitFailure(String),
    Fatal(String),
}

pub(super) fn write_daemon_startup_artifact(
    artifact_dir: Option<&Path>,
    pid: u32,
    diagnostic: &str,
) -> Result<Option<PathBuf>, String> {
    let Some(artifact_dir) = artifact_dir else {
        return Ok(None);
    };
    let artifact_path = artifact_dir.join(format!("daemon-startup-{pid}.txt"));
    fs::create_dir_all(artifact_dir).map_err(|error| {
        format!(
            "failed to persist daemon startup diagnostics at {}: {error}",
            artifact_path.display()
        )
    })?;
    fs::write(&artifact_path, diagnostic).map_err(|error| {
        format!(
            "failed to persist daemon startup diagnostics at {}: {error}",
            artifact_path.display()
        )
    })?;
    Ok(Some(artifact_path))
}

#[allow(clippy::zombie_processes)]
pub(super) fn start_daemon_for_repo(repo: &TestRepo) {
    let daemon_home = repo.daemon_home_path();
    let control_socket_path = daemon_control_socket_path(repo);
    let trace_socket_path = daemon_trace_socket_path(repo);
    let mut command = Command::new(get_binary_path());
    command
        .arg("bg")
        .arg("run")
        .current_dir(repo.path())
        .env("GIT_AI_TEST_DB_PATH", repo.test_db_path())
        .env("GITAI_TEST_DB_PATH", repo.test_db_path())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_test_home_env(&mut command, repo.test_home_path());
    configure_test_daemon_env(
        &mut command,
        &daemon_home,
        &control_socket_path,
        &trace_socket_path,
    );
    command.spawn().expect("failed to spawn daemon for repo");

    let repo_workdir = repo_workdir_string(repo);
    for _ in 0..200 {
        if send_control_request(
            &control_socket_path,
            &ControlRequest::StatusFamily {
                repo_working_dir: repo_workdir.clone(),
            },
        )
        .is_ok()
            && local_socket_connects_with_timeout(&trace_socket_path, DAEMON_TEST_PROBE_TIMEOUT)
                .is_ok()
        {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!(
        "daemon did not become ready at {}",
        control_socket_path.display()
    );
}

pub(super) struct DaemonGuard {
    pub(super) child: Child,
    pub(super) control_socket_path: PathBuf,
    pub(super) trace_socket_path: PathBuf,
    pub(super) repo_working_dir: String,
    pub(super) diagnostic_log_path: PathBuf,
}

impl DaemonGuard {
    pub(super) fn start(repo: &TestRepo) -> Self {
        Self::start_with_env(repo, &[])
    }

    pub(super) fn start_with_env(repo: &TestRepo, extra_env: &[(&str, &str)]) -> Self {
        let daemon_home = repo.daemon_home_path();
        let control_socket_path = daemon_control_socket_path(repo);
        let trace_socket_path = daemon_trace_socket_path(repo);
        let stderr_log_path = daemon_home.join("daemon-guard.stderr.log");
        fs::create_dir_all(&daemon_home).expect("failed to create daemon test home");
        let stderr_log = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&stderr_log_path)
            .expect("failed to create daemon stderr log");
        let mut command = Command::new(get_binary_path());
        command
            .arg("bg")
            .arg("run")
            .current_dir(repo.path())
            .env("GIT_AI_TEST_DB_PATH", repo.test_db_path())
            .env("GITAI_TEST_DB_PATH", repo.test_db_path())
            .stdout(Stdio::null())
            .stderr(
                stderr_log
                    .try_clone()
                    .expect("failed to clone daemon stderr log"),
            );
        for (key, value) in extra_env {
            command.env(key, value);
        }
        configure_test_home_env(&mut command, repo.test_home_path());
        configure_test_daemon_env(
            &mut command,
            &daemon_home,
            &control_socket_path,
            &trace_socket_path,
        );

        // Respawn loop: a Windows `STATUS_DLL_INIT_FAILED` exit means the OS
        // loader never started the daemon process (a hosted-Windows-runner
        // hiccup), so retry. Any other early exit / timeout panics immediately.
        let mut attempt = 0;
        let artifact_dir = std::env::var_os("GIT_AI_TEST_ARTIFACT_DIR").map(PathBuf::from);
        loop {
            let child = command.spawn().expect("failed to spawn git-ai subprocess");
            #[cfg(windows)]
            let diagnostic_log_path = daemon_log_dir(&DaemonConfig::from_home(&daemon_home))
                .join(format!("{}.log", child.id()));
            #[cfg(not(windows))]
            let diagnostic_log_path = stderr_log_path.clone();
            let mut daemon = Self {
                child,
                control_socket_path: control_socket_path.clone(),
                trace_socket_path: trace_socket_path.clone(),
                repo_working_dir: repo_workdir_string(repo),
                diagnostic_log_path,
            };
            match daemon.wait_until_ready(DAEMON_TEST_READY_TOTAL_TIMEOUT, artifact_dir.as_deref())
            {
                Ok(()) => return daemon,
                Err(DaemonReadyOutcome::LoaderInitFailure(message)) => {
                    let _ = daemon.child.kill();
                    let _ = daemon.child.wait();
                    attempt += 1;
                    if attempt < DAEMON_SPAWN_LOADER_RETRY_ATTEMPTS {
                        eprintln!(
                            "[test-harness] daemon loader init failed (attempt {}/{}), respawning: {}",
                            attempt, DAEMON_SPAWN_LOADER_RETRY_ATTEMPTS, message
                        );
                        continue;
                    }
                    panic!("{}", message);
                }
                Err(DaemonReadyOutcome::Fatal(message)) => {
                    let _ = daemon.child.kill();
                    let _ = daemon.child.wait();
                    panic!("{}", message);
                }
            }
        }
    }

    pub(super) fn wait_until_ready(
        &mut self,
        readiness_timeout: Duration,
        artifact_dir: Option<&Path>,
    ) -> Result<(), DaemonReadyOutcome> {
        let started = Instant::now();
        let mut last_control_error = None;
        let mut last_trace_error = None;
        while started.elapsed() < readiness_timeout {
            if let Some(status) = self
                .child
                .try_wait()
                .expect("failed to poll daemon process status")
            {
                let message = format!(
                    "daemon exited before becoming ready (pid {}, status {}): sockets {} {} (last_control_error={}, last_trace_error={})",
                    self.child.id(),
                    status,
                    self.control_socket_path.display(),
                    self.trace_socket_path.display(),
                    last_control_error.as_deref().unwrap_or("none"),
                    last_trace_error.as_deref().unwrap_or("none")
                );
                if is_windows_loader_init_failure(&status) {
                    return Err(DaemonReadyOutcome::LoaderInitFailure(message));
                }
                return Err(self.startup_failure(message, artifact_dir));
            }
            let control_ready = match send_control_request_with_timeout(
                &self.control_socket_path,
                &ControlRequest::StatusFamily {
                    repo_working_dir: self.repo_working_dir.clone(),
                },
                DAEMON_TEST_READY_CONTROL_TIMEOUT,
            ) {
                Ok(_) => true,
                Err(error) => {
                    last_control_error = Some(error.to_string());
                    false
                }
            };
            let trace_ready = match local_socket_connects_with_timeout(
                &self.trace_socket_path,
                DAEMON_TEST_PROBE_TIMEOUT,
            ) {
                Ok(()) => true,
                Err(error) => {
                    last_trace_error = Some(error.to_string());
                    false
                }
            };
            if control_ready && trace_ready {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(self.startup_failure(
            format!(
                "daemon did not become ready within {:?} (pid {}) at {} (trace socket: {}, last_control_error={}, last_trace_error={})",
                readiness_timeout,
                self.child.id(),
                self.control_socket_path.display(),
                self.trace_socket_path.display(),
                last_control_error.as_deref().unwrap_or("none"),
                last_trace_error.as_deref().unwrap_or("none")
            ),
            artifact_dir,
        ))
    }

    pub(super) fn startup_failure(
        &self,
        message: String,
        artifact_dir: Option<&Path>,
    ) -> DaemonReadyOutcome {
        let diagnostic = match fs::read_to_string(&self.diagnostic_log_path) {
            Ok(contents) => format!(
                "{message}\nDaemon startup diagnostics ({}):\n{contents}",
                self.diagnostic_log_path.display()
            ),
            Err(error) => format!(
                "{message}\nfailed to read daemon startup diagnostics at {}: {error}",
                self.diagnostic_log_path.display()
            ),
        };
        match write_daemon_startup_artifact(artifact_dir, self.child.id(), &diagnostic) {
            Ok(Some(path)) => DaemonReadyOutcome::Fatal(format!(
                "{diagnostic}\nDaemon startup diagnostics persisted at {}",
                path.display()
            )),
            Ok(None) => DaemonReadyOutcome::Fatal(diagnostic),
            Err(error) => DaemonReadyOutcome::Fatal(format!("{diagnostic}\n{error}")),
        }
    }

    pub(super) fn shutdown(&mut self) {
        if self
            .child
            .try_wait()
            .expect("failed polling daemon process")
            .is_some()
        {
            return;
        }

        let _ = send_control_request(&self.control_socket_path, &ControlRequest::Shutdown);

        for _ in 0..200 {
            if self
                .child
                .try_wait()
                .expect("failed polling daemon process")
                .is_some()
            {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }

        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    pub(super) fn diagnostic_contents(&self) -> String {
        fs::read_to_string(&self.diagnostic_log_path).unwrap_or_else(|error| {
            panic!(
                "failed to read daemon diagnostics at {}: {error}",
                self.diagnostic_log_path.display()
            )
        })
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(super) fn unready_daemon_guard(repo: &TestRepo, diagnostic_log_path: PathBuf) -> DaemonGuard {
    let child = Command::new(real_git_executable())
        .args(["hash-object", "--stdin"])
        .current_dir(repo.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn blocking git subprocess");
    DaemonGuard {
        child,
        control_socket_path: repo.path().join("missing-control"),
        trace_socket_path: repo.path().join("missing-trace"),
        repo_working_dir: repo_workdir_string(repo),
        diagnostic_log_path,
    }
}

pub(super) fn stop_unready_daemon(daemon: &mut DaemonGuard) {
    daemon
        .child
        .kill()
        .expect("failed to stop blocking git subprocess");
    daemon
        .child
        .wait()
        .expect("failed to reap blocking git subprocess");
}

#[test]
fn daemon_guard_readiness_timeout_persists_channel_and_daemon_diagnostics() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let diagnostic_log_path = repo.path().join("daemon.log");
    fs::write(&diagnostic_log_path, "daemon boot diagnostic\n")
        .expect("failed to write daemon diagnostic fixture");
    let artifact_dir = repo.path().join("artifacts");
    let mut daemon = unready_daemon_guard(&repo, diagnostic_log_path.clone());
    let pid = daemon.child.id();

    let outcome = daemon.wait_until_ready(Duration::from_millis(100), Some(&artifact_dir));
    stop_unready_daemon(&mut daemon);

    let DaemonReadyOutcome::Fatal(message) = outcome.expect_err("readiness should time out") else {
        panic!("readiness timeout should be fatal");
    };
    assert!(message.contains("last_control_error="));
    assert!(!message.contains("last_control_error=none"));
    assert!(message.contains("last_trace_error="));
    assert!(!message.contains("last_trace_error=none"));
    assert!(message.contains(&diagnostic_log_path.display().to_string()));
    assert!(message.contains("daemon boot diagnostic"));

    let artifact = artifact_dir.join(format!("daemon-startup-{pid}.txt"));
    let artifact_contents =
        fs::read_to_string(&artifact).expect("startup artifact should be persisted");
    assert!(artifact_contents.contains("last_control_error="));
    assert!(artifact_contents.contains("last_trace_error="));
    assert!(artifact_contents.contains("daemon boot diagnostic"));
}

#[test]
fn daemon_guard_readiness_timeout_reports_diagnostic_read_failure() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let diagnostic_log_path = repo.path().join("missing-daemon.log");
    let mut daemon = unready_daemon_guard(&repo, diagnostic_log_path.clone());

    let outcome = daemon.wait_until_ready(Duration::ZERO, None);
    stop_unready_daemon(&mut daemon);

    let DaemonReadyOutcome::Fatal(message) = outcome.expect_err("readiness should time out") else {
        panic!("readiness timeout should be fatal");
    };
    assert!(message.contains("failed to read daemon startup diagnostics"));
    assert!(message.contains(&diagnostic_log_path.display().to_string()));
}

#[test]
fn daemon_guard_readiness_timeout_reports_artifact_write_failure() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let diagnostic_log_path = repo.path().join("daemon.log");
    fs::write(&diagnostic_log_path, "daemon boot diagnostic\n")
        .expect("failed to write daemon diagnostic fixture");
    let artifact_dir = repo.path().join("artifact-file");
    fs::write(&artifact_dir, "not a directory\n").expect("failed to write artifact fixture");
    let mut daemon = unready_daemon_guard(&repo, diagnostic_log_path);
    let pid = daemon.child.id();
    let artifact_path = artifact_dir.join(format!("daemon-startup-{pid}.txt"));

    let outcome = daemon.wait_until_ready(Duration::ZERO, Some(&artifact_dir));
    stop_unready_daemon(&mut daemon);

    let DaemonReadyOutcome::Fatal(message) = outcome.expect_err("readiness should time out") else {
        panic!("readiness timeout should be fatal");
    };
    assert!(message.contains("daemon boot diagnostic"));
    assert!(message.contains("failed to persist daemon startup diagnostics"));
    assert!(message.contains(&artifact_path.display().to_string()));
}

#[test]
fn daemon_guard_early_exit_persists_channel_and_daemon_diagnostics() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let diagnostic_log_path = repo.path().join("daemon.log");
    fs::write(&diagnostic_log_path, "daemon exited diagnostic\n")
        .expect("failed to write daemon diagnostic fixture");
    let artifact_dir = repo.path().join("artifacts");
    let child = Command::new(real_git_executable())
        .args(["-c", "alias.delay=!sleep 1; exit 7", "delay"])
        .current_dir(repo.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn delayed-exit git subprocess");
    let pid = child.id();
    let mut daemon = DaemonGuard {
        child,
        control_socket_path: repo.path().join("missing-control"),
        trace_socket_path: repo.path().join("missing-trace"),
        repo_working_dir: repo_workdir_string(&repo),
        diagnostic_log_path,
    };

    let outcome = daemon.wait_until_ready(Duration::from_secs(3), Some(&artifact_dir));
    daemon
        .child
        .wait()
        .expect("failed to reap delayed-exit git subprocess");

    let DaemonReadyOutcome::Fatal(message) = outcome.expect_err("daemon exit should be fatal")
    else {
        panic!("non-loader daemon exit should be fatal");
    };
    assert!(message.contains("daemon exited before becoming ready"));
    assert!(message.contains("last_control_error="));
    assert!(!message.contains("last_control_error=none"));
    assert!(message.contains("last_trace_error="));
    assert!(!message.contains("last_trace_error=none"));
    assert!(message.contains("daemon exited diagnostic"));

    let artifact = artifact_dir.join(format!("daemon-startup-{pid}.txt"));
    let artifact_contents =
        fs::read_to_string(&artifact).expect("startup artifact should be persisted");
    assert!(artifact_contents.contains("last_control_error="));
    assert!(artifact_contents.contains("last_trace_error="));
    assert!(artifact_contents.contains("daemon exited diagnostic"));
}
