use super::*;
use crate::error::GitAiError;
use serde_json::Value;
use std::io::Read;
use std::sync::Arc;

#[cfg(not(windows))]
pub enum TraceConnectionBootstrap {
    Continue,
    Stop,
    Eof,
}

pub struct TraceLineOutcome {
    continue_reading: bool,
    #[cfg(not(windows))]
    bootstrap_complete: bool,
}

#[cfg(not(windows))]
pub const TRACE_CONNECTION_BOOTSTRAP_MAX_LINES: usize = 8;

#[cfg(not(windows))]
pub fn bootstrap_trace_connection_actor_reader<R: Read>(
    reader: &mut TraceReader<R>,
    coordinator: Arc<ActorDaemonCoordinator>,
    observed_roots: &mut std::collections::BTreeSet<String>,
) -> Result<TraceConnectionBootstrap, GitAiError> {
    for _ in 0..TRACE_CONNECTION_BOOTSTRAP_MAX_LINES {
        let line = match read_trace_line(reader, &coordinator) {
            Ok(Some(line)) => line,
            Ok(None) => return Ok(TraceConnectionBootstrap::Eof),
            Err(error) if trace_bootstrap_read_timed_out(&error) => {
                return Ok(TraceConnectionBootstrap::Continue);
            }
            Err(error) => return Err(error),
        };
        let Some(outcome) =
            process_trace_connection_line(&line, coordinator.clone(), observed_roots)?
        else {
            continue;
        };
        if !outcome.continue_reading {
            return Ok(TraceConnectionBootstrap::Stop);
        }
        if outcome.bootstrap_complete {
            return Ok(TraceConnectionBootstrap::Continue);
        }
    }
    Ok(TraceConnectionBootstrap::Continue)
}

#[cfg(not(windows))]
pub fn trace_bootstrap_read_timed_out(error: &GitAiError) -> bool {
    matches!(
        error,
        GitAiError::IoError(io_error)
            if matches!(
                io_error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            )
    )
}

pub fn handle_trace_connection_actor_reader<R: Read>(
    mut reader: TraceReader<R>,
    coordinator: Arc<ActorDaemonCoordinator>,
    mut observed_roots: std::collections::BTreeSet<String>,
) -> Result<(), GitAiError> {
    let result = (|| {
        while let Some(line) = read_trace_line(&mut reader, &coordinator)? {
            if process_trace_connection_line(&line, coordinator.clone(), &mut observed_roots)?
                .is_some_and(|outcome| !outcome.continue_reading)
            {
                break;
            }
        }
        Ok(())
    })();
    let cleanup = finalize_trace_connection_roots(coordinator, observed_roots);
    result.and(cleanup)
}

pub fn process_trace_connection_line(
    line: &str,
    coordinator: Arc<ActorDaemonCoordinator>,
    observed_roots: &mut std::collections::BTreeSet<String>,
) -> Result<Option<TraceLineOutcome>, GitAiError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let mut parsed: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if parsed.get("event").and_then(Value::as_str)
        == Some(super::socket_health::TRACE_HEALTH_PING_EVENT)
        && parsed.get("sid").is_none()
    {
        coordinator
            .trace_health_pings_received
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Ok(Some(TraceLineOutcome {
            continue_reading: false,
            #[cfg(not(windows))]
            bootstrap_complete: false,
        }));
    }
    #[cfg(not(windows))]
    let event = parsed
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    #[cfg(not(windows))]
    let mut bootstrap_complete = false;
    if let Some(sid) = parsed.get("sid").and_then(Value::as_str) {
        let was_unidentified = observed_roots.is_empty();
        let root_sid = trace_root_sid(sid).to_string();
        // `start` carries argv but not the worktree. Keep bootstrapping on the
        // listener thread until the root `def_repo` event has been processed;
        // that is the first point where trace augmentation can capture reflog
        // start offsets with a concrete worktree.
        #[cfg(not(windows))]
        if event == "def_repo" && sid == root_sid {
            bootstrap_complete = true;
        }
        if observed_roots.insert(root_sid.clone()) {
            let _ = coordinator.trace_root_connection_opened(&root_sid);
        }
        if was_unidentified {
            coordinator.trace_unidentified_connection_identified_or_closed()?;
        }
    }
    // Only enqueue payloads for mutating commands.  Read-only invocations
    // (status, diff, stash list, worktree list, …) are handled inline by
    // prepare_trace_payload_for_ingest and must not enter the serial ingest
    // queue — doing so causes the >1-minute backlog seen with IDEs that
    // issue dozens of read-only git commands per second.
    let continue_reading = !(coordinator.prepare_trace_payload_for_ingest(&mut parsed)
        && coordinator.enqueue_trace_payload(parsed).is_err());
    Ok(Some(TraceLineOutcome {
        continue_reading,
        #[cfg(not(windows))]
        bootstrap_complete,
    }))
}

fn read_trace_line<R: Read>(
    reader: &mut TraceReader<R>,
    coordinator: &ActorDaemonCoordinator,
) -> Result<Option<String>, GitAiError> {
    let result = reader.read_line();
    if let Err(GitAiError::IoError(error)) = &result
        && error.kind() == std::io::ErrorKind::InvalidData
    {
        // Losing a trace frame invalidates causal evidence just like a full
        // ingest queue. Stop admission before connection cleanup releases it.
        tracing::error!(%error, "invalid trace frame; requesting shutdown");
        coordinator.request_shutdown();
    }
    result
}

#[cfg(test)]
mod tests;
