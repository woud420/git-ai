use super::*;

struct DetachedDaemonCleanup<'a>(&'a TestRepo);

impl Drop for DetachedDaemonCleanup<'_> {
    fn drop(&mut self) {
        let _ = bg_command(self.0, "shutdown", &["--hard"]);
    }
}

fn captured_pipe(
    mut pipe: impl std::io::Read + Send + 'static,
) -> std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = pipe.read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    receiver
}

#[test]
fn windows_daemon_start_without_powershell_closes_captured_pipes() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let empty_path = tempfile::tempdir().unwrap();
    assert!(
        Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "exit 0"])
            .env("PATH", empty_path.path())
            .output()
            .is_err(),
        "fixture must prevent PowerShell resolution",
    );
    let _cleanup = DetachedDaemonCleanup(&repo);
    let git = real_git_executable();
    let mut command = repo
        .git_ai_command_without_pre_sync_for_test(&["bg", "start"], &[("GIT_AI_GIT_PATH", git)]);
    command
        .env("PATH", empty_path.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("start wrapper should launch");
    let stdout = captured_pipe(child.stdout.take().unwrap());
    let stderr = captured_pipe(child.stderr.take().unwrap());
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("bg start wrapper did not exit within 20 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout
        .recv_timeout(Duration::from_secs(2))
        .expect("wrapper stdout must reach EOF while daemon remains alive")
        .unwrap();
    let stderr = stderr
        .recv_timeout(Duration::from_secs(2))
        .expect("wrapper stderr must reach EOF while daemon remains alive")
        .unwrap();
    assert!(
        status.success(),
        "bg start requires PowerShell: stdout={} stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    let config = DaemonConfig::from_home(&repo.daemon_home_path());
    let pid = read_daemon_pid(&config).expect("daemon should remain running");
    assert!(process_exists(pid));
    let response = send_control_request(&daemon_control_socket_path(&repo), &ControlRequest::Ping)
        .expect("detached daemon must respond after wrapper and pipes close");
    assert!(response.ok, "{response:?}");
}
