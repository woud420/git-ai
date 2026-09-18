use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

mod output;
#[cfg(windows)]
mod windows;
use output::{
    OutputEvent, OutputState, collect_output_until, drain_output_events, spawn_output_reader,
};

const OUTPUT_DRAIN_GRACE: Duration = Duration::from_millis(200);
const OUTPUT_DRAIN_POLL: Duration = Duration::from_millis(10);

#[cfg(all(test, unix))]
mod tests;

#[derive(Debug, Clone)]
pub(crate) struct TimedCommandOutput<Buffer = String> {
    pub status: Option<i32>,
    pub stdout: Buffer,
    pub stderr: Buffer,
    pub timed_out: bool,
    pub diagnostics: Vec<String>,
    pub wait_error: Option<String>,
}

struct PipedChild {
    child: Child,
    #[cfg(windows)]
    job: windows::Job,
    rx: Receiver<OutputEvent>,
    output: OutputState,
}

pub(crate) fn run_command_with_timeout(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout: Duration,
    poll_interval: Duration,
    env_remove: &[&str],
) -> Result<TimedCommandOutput, String> {
    run_command_with_timeout_and_env(program, args, cwd, timeout, poll_interval, env_remove, &[])
}

pub(crate) fn run_command_with_timeout_and_env(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout: Duration,
    poll_interval: Duration,
    env_remove: &[&str],
    env_set: &[(&str, &str)],
) -> Result<TimedCommandOutput, String> {
    let mut command = Command::new(program);
    command.args(args);
    for key in env_remove {
        command.env_remove(key);
    }
    for (key, value) in env_set {
        command.env(key, value);
    }
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    run_prepared_command_with_timeout(command, timeout, poll_interval)
        .map(TimedCommandOutput::into_text)
        .map_err(|error| format!("failed to execute: {error}"))
}

pub(crate) fn run_prepared_command_with_timeout(
    mut command: Command,
    timeout: Duration,
    poll_interval: Duration,
) -> std::io::Result<TimedCommandOutput<Vec<u8>>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let running = spawn_piped(command)?;
    Ok(wait_with_timeout(running, timeout, poll_interval))
}

impl TimedCommandOutput<Vec<u8>> {
    fn into_text(self) -> TimedCommandOutput {
        TimedCommandOutput {
            status: self.status,
            stdout: String::from_utf8_lossy(&self.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&self.stderr).trim().to_string(),
            timed_out: self.timed_out,
            diagnostics: self.diagnostics,
            wait_error: self.wait_error,
        }
    }
}

fn spawn_piped(mut command: Command) -> std::io::Result<PipedChild> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    let (mut child, job) = windows::spawn(&mut command)?;
    #[cfg(not(windows))]
    let mut child = command.spawn()?;

    let (tx, rx) = mpsc::channel();
    let mut output = OutputState::default();
    match child.stdout.take() {
        Some(stdout) => spawn_output_reader(stdout, tx.clone(), true),
        None => output.stdout_done = true,
    }
    match child.stderr.take() {
        Some(stderr) => spawn_output_reader(stderr, tx.clone(), false),
        None => output.stderr_done = true,
    }
    drop(tx);

    Ok(PipedChild {
        child,
        #[cfg(windows)]
        job,
        rx,
        output,
    })
}

fn wait_with_timeout(
    running: PipedChild,
    timeout: Duration,
    poll_interval: Duration,
) -> TimedCommandOutput<Vec<u8>> {
    let PipedChild {
        mut child,
        #[cfg(windows)]
        job,
        rx,
        mut output,
    } = running;
    let start = Instant::now();
    loop {
        drain_output_events(&rx, &mut output);
        match child.try_wait() {
            Ok(Some(status)) => {
                collect_output_until(
                    &rx,
                    &mut output,
                    Instant::now() + OUTPUT_DRAIN_GRACE,
                    OUTPUT_DRAIN_POLL,
                );
                if !output.complete() {
                    output.diagnostics.push(
                        "output collection did not finish after the child exited; descendant processes may still be holding stdout/stderr open".to_string(),
                    );
                }
                return output.finish(status.code(), false, None);
            }
            Ok(None) if start.elapsed() >= timeout => {
                #[cfg(windows)]
                job.terminate(&mut output.diagnostics);
                kill_process_group(&child, &mut output.diagnostics);
                let kill_result = child.kill();
                match &kill_result {
                    Ok(()) => output
                        .diagnostics
                        .push("sent kill to child process".to_string()),
                    Err(e) => output
                        .diagnostics
                        .push(format!("failed to kill child process: {}", e)),
                }

                let wait_result = child.wait();
                let status = match wait_result {
                    Ok(status) => {
                        output.diagnostics.push(format!(
                            "child process exited after timeout with status {}",
                            status
                                .code()
                                .map(|code| code.to_string())
                                .unwrap_or_else(|| "signal".to_string())
                        ));
                        status.code()
                    }
                    Err(e) => {
                        output
                            .diagnostics
                            .push(format!("failed to wait for child after timeout: {}", e));
                        None
                    }
                };

                collect_output_until(
                    &rx,
                    &mut output,
                    Instant::now() + OUTPUT_DRAIN_GRACE,
                    OUTPUT_DRAIN_POLL,
                );
                if !output.complete() {
                    output.diagnostics.push(
                        "output collection incomplete after timeout; descendant processes may still be holding stdout/stderr open".to_string(),
                    );
                }
                return output.finish(status, true, None);
            }
            Ok(None) => {
                std::thread::sleep(poll_interval);
            }
            Err(e) => {
                #[cfg(windows)]
                job.terminate(&mut output.diagnostics);
                kill_process_group(&child, &mut output.diagnostics);
                let _ = child.kill();
                let _ = child.wait();
                collect_output_until(
                    &rx,
                    &mut output,
                    Instant::now() + OUTPUT_DRAIN_GRACE,
                    OUTPUT_DRAIN_POLL,
                );
                return output.finish(None, false, Some(e.to_string()));
            }
        }
    }
}

fn kill_process_group(child: &Child, diagnostics: &mut Vec<String>) {
    #[cfg(unix)]
    {
        // Every child from spawn_piped leads its own group; include SSH and
        // credential helpers that would otherwise retain the capture pipes.
        if unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) } == 0 {
            diagnostics.push("sent kill to child process group".to_string());
        }
    }
    #[cfg(not(unix))]
    let _ = (child, diagnostics);
}

#[cfg(test)]
const FIXTURE_READY_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
pub(crate) fn partial_output_fixture(timeout: Duration) -> Result<TimedCommandOutput, String> {
    let (program, args) = partial_output_command();
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut running =
        spawn_piped(command).map_err(|error| format!("failed to execute: {error}"))?;
    let deadline = Instant::now() + FIXTURE_READY_TIMEOUT;
    loop {
        drain_output_events(&running.rx, &mut running.output);
        if running.output.stdout == b"out" && running.output.stderr == b"err" {
            return Ok(wait_with_timeout(running, timeout, OUTPUT_DRAIN_POLL).into_text());
        }

        match running.child.try_wait() {
            Ok(Some(status)) => {
                collect_output_until(
                    &running.rx,
                    &mut running.output,
                    Instant::now() + OUTPUT_DRAIN_GRACE,
                    OUTPUT_DRAIN_POLL,
                );
                return Err(format!(
                    "partial-output fixture exited before ready (status={:?}, stdout={:?}, stderr={:?})",
                    status.code(),
                    String::from_utf8_lossy(&running.output.stdout),
                    String::from_utf8_lossy(&running.output.stderr)
                ));
            }
            Err(error) => {
                let cleanup = stop_fixture(&mut running)
                    .err()
                    .map(|error| format!("; {}", error))
                    .unwrap_or_default();
                return Err(format!(
                    "failed while waiting for partial-output fixture: {}{}",
                    error, cleanup
                ));
            }
            Ok(None) if Instant::now() >= deadline => {
                let cleanup = stop_fixture(&mut running)
                    .err()
                    .map(|error| format!("; {}", error))
                    .unwrap_or_default();
                return Err(format!(
                    "partial-output fixture was not ready (stdout={:?}, stderr={:?}){}",
                    String::from_utf8_lossy(&running.output.stdout),
                    String::from_utf8_lossy(&running.output.stderr),
                    cleanup
                ));
            }
            Ok(None) => std::thread::sleep(OUTPUT_DRAIN_POLL),
        }
    }
}

#[cfg(test)]
fn stop_fixture(running: &mut PipedChild) -> Result<(), String> {
    match running.child.kill() {
        Ok(()) => running
            .child
            .wait()
            .map(|_| ())
            .map_err(|error| format!("failed to reap partial-output fixture: {}", error)),
        Err(kill_error) => match running.child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(format!(
                "failed to stop partial-output fixture: {}",
                kill_error
            )),
            Err(wait_error) => Err(format!(
                "failed to stop partial-output fixture: {}; status check also failed: {}",
                kill_error, wait_error
            )),
        },
    }
}

#[cfg(all(test, not(windows)))]
fn partial_output_command() -> (&'static str, [&'static str; 2]) {
    ("sh", ["-c", "printf out; printf err >&2; exec sleep 60"])
}

#[cfg(all(test, windows))]
fn partial_output_command() -> (&'static str, [&'static str; 3]) {
    (
        "powershell.exe",
        [
            "-NoProfile",
            "-Command",
            "[Console]::Out.Write('out'); [Console]::Out.Flush(); [Console]::Error.Write('err'); [Console]::Error.Flush(); Start-Sleep -Seconds 60",
        ],
    )
}
