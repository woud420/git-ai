use super::TimedCommandOutput;
use std::io::Read;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

pub(super) enum OutputEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    StdoutDone,
    StderrDone,
    StdoutError(String),
    StderrError(String),
}

#[derive(Default)]
pub(super) struct OutputState {
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) stdout_done: bool,
    pub(super) stderr_done: bool,
    pub(super) diagnostics: Vec<String>,
}

impl OutputState {
    pub(super) fn complete(&self) -> bool {
        self.stdout_done && self.stderr_done
    }

    pub(super) fn finish(
        self,
        status: Option<i32>,
        timed_out: bool,
        wait_error: Option<String>,
    ) -> TimedCommandOutput<Vec<u8>> {
        TimedCommandOutput {
            status,
            stdout: self.stdout,
            stderr: self.stderr,
            timed_out,
            diagnostics: self.diagnostics,
            wait_error,
        }
    }
}

pub(super) fn spawn_output_reader<R>(mut reader: R, tx: Sender<OutputEvent>, stdout: bool)
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buf = [0_u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let event = if stdout {
                        OutputEvent::Stdout(buf[..n].to_vec())
                    } else {
                        OutputEvent::Stderr(buf[..n].to_vec())
                    };
                    if tx.send(event).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let event = if stdout {
                        OutputEvent::StdoutError(e.to_string())
                    } else {
                        OutputEvent::StderrError(e.to_string())
                    };
                    let _ = tx.send(event);
                    return;
                }
            }
        }

        let event = if stdout {
            OutputEvent::StdoutDone
        } else {
            OutputEvent::StderrDone
        };
        let _ = tx.send(event);
    });
}

pub(super) fn collect_output_until(
    rx: &Receiver<OutputEvent>,
    output: &mut OutputState,
    deadline: Instant,
    poll_interval: Duration,
) {
    while !output.complete() && Instant::now() < deadline {
        drain_output_events(rx, output);
        if output.complete() {
            break;
        }
        std::thread::sleep(poll_interval);
    }
    drain_output_events(rx, output);
}

pub(super) fn drain_output_events(rx: &Receiver<OutputEvent>, output: &mut OutputState) {
    // A continuously writing child must not starve the caller's deadline check.
    for _ in 0..256 {
        let Ok(event) = rx.try_recv() else {
            break;
        };
        match event {
            OutputEvent::Stdout(bytes) => output.stdout.extend(bytes),
            OutputEvent::Stderr(bytes) => output.stderr.extend(bytes),
            OutputEvent::StdoutDone => output.stdout_done = true,
            OutputEvent::StderrDone => output.stderr_done = true,
            OutputEvent::StdoutError(err) => {
                output
                    .diagnostics
                    .push(format!("failed to read stdout: {}", err));
                output.stdout_done = true;
            }
            OutputEvent::StderrError(err) => {
                output
                    .diagnostics
                    .push(format!("failed to read stderr: {}", err));
                output.stderr_done = true;
            }
        }
    }
}
