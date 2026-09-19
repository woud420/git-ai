use super::*;
use std::io::Write;

#[test]
fn timed_commands_do_not_consume_the_callers_stdin() {
    const PROBE: &str = "GIT_AI_TEST_TIMED_STDIN_PROBE";
    if std::env::var_os(PROBE).is_some() {
        let output = run_command_with_timeout(
            "sh",
            &[
                "-c",
                "if read line; then echo \"$line\"; else echo closed; fi",
            ],
            None,
            Duration::from_secs(2),
            Duration::from_millis(10),
            &[],
        )
        .unwrap();
        assert_eq!(output.stdout, "closed");
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process_timeout::tests::timed_commands_do_not_consume_the_callers_stdin",
            "--nocapture",
        ])
        .env(PROBE, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"open\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn timeout_terminates_descendants_holding_captured_pipes() {
    let output = run_command_with_timeout(
        "sh",
        &["-c", "sleep 30 & echo $!; wait"],
        None,
        Duration::from_millis(300),
        Duration::from_millis(10),
        &[],
    )
    .unwrap();
    let descendant: i32 = output
        .stdout
        .parse()
        .expect("fixture must report its own child PID");
    assert!(descendant > 0);
    // The failing baseline strands a live sleeper. A successful group kill
    // already owns cleanup; do not signal a PID that might then be recycled.
    if !output
        .diagnostics
        .iter()
        .any(|message| message.contains("kill to child process group"))
    {
        unsafe {
            libc::kill(descendant, libc::SIGKILL);
        }
    }
    assert!(output.timed_out);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|message| message.contains("kill to child process group")),
        "{output:?}"
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|message| message.contains("incomplete")),
        "{output:?}"
    );
}

#[test]
fn prepared_commands_preserve_raw_output_for_git_error_matching() {
    let mut command = Command::new("sh");
    command.args(["-c", "printf ' out\\n'; printf ' err\\377\\n' >&2; exit 17"]);
    let output = run_prepared_command_with_timeout(
        command,
        Duration::from_secs(2),
        Duration::from_millis(10),
    )
    .unwrap();
    assert_eq!(output.status, Some(17));
    assert_eq!(output.stdout, b" out\n");
    assert_eq!(output.stderr, b" err\xff\n");
}

#[test]
fn continuous_output_does_not_starve_the_timeout() {
    let start = Instant::now();
    let output = run_command_with_timeout(
        "sh",
        &["-c", "while :; do printf 'busy\\n'; done"],
        None,
        Duration::from_millis(100),
        Duration::from_millis(10),
        &[],
    )
    .unwrap();
    assert!(output.timed_out);
    assert!(!output.stdout.is_empty());
    assert!(start.elapsed() < Duration::from_secs(2));
}
