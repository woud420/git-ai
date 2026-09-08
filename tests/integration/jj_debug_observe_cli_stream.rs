use std::collections::VecDeque;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const WATCHDOG: Duration = Duration::from_secs(20);
const LINE_LIMIT: usize = 128 * 1024;

pub(super) struct Stream {
    child: Child,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    partial: Vec<u8>,
    lines: VecDeque<Vec<u8>>,
    diagnostics: Vec<u8>,
    bytes: usize,
    records: usize,
    delivered: usize,
    started: Instant,
    previous: Instant,
}

impl Stream {
    pub(super) fn spawn(mut command: Command) -> Self {
        let started = Instant::now();
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            child,
            partial: Vec::new(),
            lines: VecDeque::new(),
            diagnostics: Vec::new(),
            bytes: 0,
            records: 0,
            delivered: 0,
            started,
            previous: started,
        }
    }

    pub(super) fn next(&mut self) -> serde_json::Value {
        let until = Instant::now() + WATCHDOG;
        loop {
            if let Some(line) = self.lines.pop_front() {
                self.delivered += 1;
                let now = Instant::now();
                eprintln!(
                    "OBSERVE_STREAM_RECORD:{} elapsed_ms={} since_previous_ms={}",
                    self.delivered,
                    now.duration_since(self.started).as_millis(),
                    now.duration_since(self.previous).as_millis()
                );
                self.previous = now;
                return serde_json::from_slice(&line).expect("one JSON value per flushed line");
            }
            assert!(
                self.stdout.is_some(),
                "observer stdout ended before its next record: {}",
                self.text()
            );
            self.pump(until);
        }
    }

    pub(super) fn running(&mut self) {
        assert!(
            self.child.try_wait().unwrap().is_none(),
            "observer exited before the next interval"
        );
    }

    pub(super) fn pause(&mut self) -> Paused<'_> {
        self.running();
        let result = unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGSTOP) };
        assert_eq!(
            result,
            0,
            "could not pause observer: {}",
            std::io::Error::last_os_error()
        );
        Paused { stream: self }
    }

    pub(super) fn close_stdout(&mut self) {
        assert!(self.lines.is_empty() && self.partial.is_empty());
        drop(self.stdout.take());
    }

    pub(super) fn finish(&mut self, success: bool) -> ExitStatus {
        let until = Instant::now() + WATCHDOG;
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            self.pump(until);
        };
        while self.stdout.is_some() || self.stderr.is_some() {
            self.pump(until);
        }
        assert!(
            self.lines.is_empty(),
            "observer emitted an extra record or final summary"
        );
        assert!(
            self.partial.is_empty(),
            "observer emitted an unterminated record"
        );
        assert_eq!(
            status.success(),
            success,
            "observer exit: {status}: {}",
            self.text()
        );
        eprintln!(
            "OBSERVE_STREAM_FINISHED: elapsed_ms={}",
            self.started.elapsed().as_millis()
        );
        status
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.diagnostics).into_owned()
    }

    fn pump(&mut self, until: Instant) {
        assert!(
            Instant::now() < until,
            "observer watchdog expired: {}",
            self.text()
        );
        let mut descriptors = [
            libc::pollfd {
                fd: self.stdout.as_ref().map_or(-1, AsRawFd::as_raw_fd),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.stderr.as_ref().map_or(-1, AsRawFd::as_raw_fd),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // Read only a ready pipe; no reader thread can outlive the guarded child.
        let ready = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, 50) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
            return;
        }
        let mut bytes = [0; 4096];
        for (index, descriptor) in descriptors.iter().enumerate() {
            if descriptor.revents == 0 {
                continue;
            }
            assert_eq!(descriptor.revents & libc::POLLNVAL, 0);
            let count = if index == 0 {
                self.stdout.as_mut().unwrap().read(&mut bytes)
            } else {
                self.stderr.as_mut().unwrap().read(&mut bytes)
            };
            let count = match count {
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("observer pipe failed: {error}"),
            };
            if count == 0 {
                if index == 0 {
                    self.stdout.take();
                } else {
                    self.stderr.take();
                }
                continue;
            }
            if index == 1 {
                assert!(self.diagnostics.len() + count <= 64 * 1024);
                self.diagnostics.extend_from_slice(&bytes[..count]);
                continue;
            }
            self.bytes += count;
            assert!(self.bytes <= 4 * 1024 * 1024);
            for byte in &bytes[..count] {
                if *byte == b'\n' {
                    self.records += 1;
                    assert!(self.records <= 33);
                    assert!(!self.partial.is_empty());
                    self.lines.push_back(std::mem::take(&mut self.partial));
                } else {
                    assert!(self.partial.len() < LINE_LIMIT);
                    self.partial.push(*byte);
                }
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub(super) struct Paused<'a> {
    stream: &'a mut Stream,
}

impl Drop for Paused<'_> {
    fn drop(&mut self) {
        // Unwind resumes first; the enclosing Stream then kills and reaps its child.
        unsafe { libc::kill(self.stream.child.id() as libc::pid_t, libc::SIGCONT) };
    }
}
