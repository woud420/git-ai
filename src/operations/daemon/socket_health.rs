use super::{
    ActorDaemonCoordinator, ControlRequest, ControlResponse, DaemonClientStream,
    open_local_socket_stream_with_timeout, set_daemon_client_stream_timeouts,
};
use crate::error::GitAiError;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub(crate) const TRACE_HEALTH_PING_EVENT: &str = "git_ai_health_ping";
const PING_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_PING_RESPONSE_BYTES: usize = 1024;

struct PingHealth {
    threshold: Duration,
    unacked_since: Option<Instant>,
    alerted: bool,
}

impl PingHealth {
    fn new(threshold: Duration) -> Self {
        Self {
            threshold,
            unacked_since: None,
            alerted: false,
        }
    }

    fn observe(&mut self, acknowledged: bool, now: Instant) -> Option<Duration> {
        if self.threshold.is_zero() || acknowledged {
            self.unacked_since = None;
            self.alerted = false;
            return None;
        }
        let since = *self.unacked_since.get_or_insert(now);
        let elapsed = now.saturating_duration_since(since);
        if elapsed < self.threshold || self.alerted {
            return None;
        }
        self.alerted = true;
        Some(elapsed)
    }
}

pub(crate) struct SocketHealthMonitor {
    control: PingHealth,
    trace: PingHealth,
    previous_trace_count: Option<u64>,
}

impl SocketHealthMonitor {
    pub(crate) fn from_env() -> Self {
        let seconds = std::env::var("GIT_AI_DAEMON_PING_STALL_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(120);
        Self {
            control: PingHealth::new(Duration::from_secs(seconds)),
            trace: PingHealth::new(Duration::from_secs(seconds)),
            previous_trace_count: None,
        }
    }

    pub(crate) fn check(
        &mut self,
        coordinator: &ActorDaemonCoordinator,
        control: &Path,
        trace: &Path,
    ) {
        if self.control.threshold.is_zero() {
            return;
        }
        let count = coordinator
            .trace_health_pings_received
            .load(Ordering::Relaxed);
        if let Some(previous) = self.previous_trace_count.replace(count)
            && let Some(stall) = self.trace.observe(count != previous, Instant::now())
        {
            tracing::warn!(
                stall_secs = stall.as_secs(),
                "daemon trace socket stopped acknowledging health pings"
            );
        }
        let acknowledged = control_ping(control).is_ok_and(|response| response.ok);
        if let Some(stall) = self.control.observe(acknowledged, Instant::now()) {
            tracing::warn!(
                stall_secs = stall.as_secs(),
                "daemon control socket stopped acknowledging health pings"
            );
        }
        if let Err(error) = write_ping(trace, b"{\"event\":\"git_ai_health_ping\"}\n") {
            tracing::debug!(%error, "trace health ping send failed");
        }
    }
}

fn remaining(deadline: Instant) -> Result<Duration, GitAiError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "socket health ping timed out").into()
        })
}

fn write_ping(
    path: &Path,
    mut payload: &[u8],
) -> Result<(DaemonClientStream, Instant), GitAiError> {
    let deadline = Instant::now() + PING_TIMEOUT;
    let mut stream = open_local_socket_stream_with_timeout(path, remaining(deadline)?)?;
    while !payload.is_empty() {
        set_daemon_client_stream_timeouts(&mut stream, path, remaining(deadline)?)?;
        let written = stream.write(payload)?;
        if written == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::WriteZero).into());
        }
        payload = &payload[written..];
    }
    // These streams are unbuffered. Windows pipe flush waits for the peer to
    // read, defeating the deadline precisely when that peer is stalled.
    Ok((stream, deadline))
}

fn control_ping(path: &Path) -> Result<ControlResponse, GitAiError> {
    let mut request = serde_json::to_vec(&ControlRequest::Ping)?;
    request.push(b'\n');
    let (mut stream, deadline) = write_ping(path, &request)?;
    let mut response = [0; MAX_PING_RESPONSE_BYTES];
    let mut received = 0;
    while received < response.len() {
        set_daemon_client_stream_timeouts(&mut stream, path, remaining(deadline)?)?;
        let read = stream.read(&mut response[received..])?;
        if read == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
        }
        received += read;
        if let Some(end) = response[..received].iter().position(|byte| *byte == b'\n') {
            return Ok(serde_json::from_slice(&response[..end])?);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "oversized socket health response",
    )
    .into())
}

#[cfg(test)]
mod tests;
