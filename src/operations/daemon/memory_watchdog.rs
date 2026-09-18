//! Daemon resident-memory watchdog.
//!
//! When `daemon_memory_limit_mb` is configured, a dedicated OS thread samples
//! the process's current RSS once per poll interval — entirely off the trace2
//! ingestion path — and aborts the daemon at 85% of the configured limit,
//! preserving headroom before the hard ceiling. Before aborting it flushes a
//! durable stderr diagnostic and gives one direct daemon-log upload a bounded
//! window to complete. It deliberately does not drain in-flight work or
//! restart the daemon: normal demand starts a fresh daemon later, and the
//! checkpoint outbox preserves durability across the abort.

mod rss;
mod sampler;

use sampler::{MemoryUsage, RssSampler};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

use crate::model::api_types::{DaemonLogEvent, DaemonLogFieldValue, DaemonLogKind, DaemonLogLevel};

use super::ActorDaemonCoordinator;
use super::telemetry_worker::{EmergencyLogUploadStatus, upload_emergency_daemon_log};

const EMERGENCY_PERCENT: u64 = 85;
const EMERGENCY_LOG_UPLOAD_TIMEOUT: Duration = Duration::from_millis(500);
const WATCHDOG_POLL_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(feature = "test-support")]
pub(super) const TEST_POLL_INTERVAL_ENV: &str = "GIT_AI_TEST_DAEMON_MEMORY_POLL_MS";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MemoryThresholds {
    pub(super) emergency_bytes: u64,
    pub(super) limit_bytes: u64,
}

impl MemoryThresholds {
    pub(super) fn from_limit_bytes(limit_bytes: u64) -> Self {
        let emergency_bytes =
            ((u128::from(limit_bytes) * u128::from(EMERGENCY_PERCENT)).div_ceil(100)) as u64;
        Self {
            emergency_bytes,
            limit_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MemoryWatchdogDecision {
    Continue,
    Abort,
}

pub(super) fn start(coordinator: Arc<ActorDaemonCoordinator>, limit_bytes: u64) -> io::Result<()> {
    let thresholds = MemoryThresholds::from_limit_bytes(limit_bytes);
    std::thread::Builder::new()
        .name("memory-watchdog".to_string())
        .spawn(move || {
            run_watchdog(coordinator, thresholds);
        })
        .map(|_| ())
}

fn run_watchdog(coordinator: Arc<ActorDaemonCoordinator>, thresholds: MemoryThresholds) {
    let mut sampler = RssSampler::new();
    let mut measurement_failed = false;
    let poll_interval = watchdog_poll_interval();

    tracing::info!(
        memory_limit_bytes = thresholds.limit_bytes,
        memory_emergency_threshold_bytes = thresholds.emergency_bytes,
        memory_poll_interval_ms = poll_interval.as_millis() as u64,
        "daemon memory watchdog started"
    );

    loop {
        std::thread::sleep(poll_interval);
        if coordinator.is_shutting_down() {
            return;
        }

        let usage = match sampler.sample() {
            Ok(bytes) => {
                if measurement_failed {
                    tracing::info!("daemon RSS measurement recovered");
                    measurement_failed = false;
                }
                bytes
            }
            Err(error) => {
                if !measurement_failed {
                    tracing::warn!(%error, "failed measuring daemon RSS; watchdog will retry");
                    measurement_failed = true;
                }
                continue;
            }
        };

        match decision_for_rss(usage.current_bytes, thresholds) {
            MemoryWatchdogDecision::Continue => {}
            MemoryWatchdogDecision::Abort => {
                record_memory_emergency(usage, thresholds, "abort");
                std::process::abort();
            }
        }
    }
}

fn record_memory_emergency(usage: MemoryUsage, thresholds: MemoryThresholds, action: &'static str) {
    tracing::error!(
        current_rss_bytes = usage.current_bytes,
        peak_rss_bytes = ?usage.peak_bytes,
        memory_emergency_threshold_bytes = thresholds.emergency_bytes,
        memory_limit_bytes = thresholds.limit_bytes,
        action,
        "daemon memory emergency threshold reached"
    );
    let peak = usage
        .peak_bytes
        .map_or_else(|| "unavailable".to_string(), |bytes| bytes.to_string());
    eprintln!(
        "[git-ai] daemon memory emergency threshold reached (current RSS {} bytes, peak RSS {peak} bytes, emergency threshold {} bytes, hard limit {} bytes); {action}ing immediately without draining",
        usage.current_bytes, thresholds.emergency_bytes, thresholds.limit_bytes
    );
    let _ = io::stderr().flush();

    let mut fields = BTreeMap::new();
    fields.insert(
        "current_rss_bytes".to_string(),
        DaemonLogFieldValue::from(usage.current_bytes),
    );
    if let Some(peak_bytes) = usage.peak_bytes {
        fields.insert(
            "peak_rss_bytes".to_string(),
            DaemonLogFieldValue::from(peak_bytes),
        );
    }
    fields.insert(
        "memory_emergency_threshold_bytes".to_string(),
        DaemonLogFieldValue::from(thresholds.emergency_bytes),
    );
    fields.insert(
        "memory_limit_bytes".to_string(),
        DaemonLogFieldValue::from(thresholds.limit_bytes),
    );
    fields.insert("action".to_string(), DaemonLogFieldValue::from(action));
    let event = DaemonLogEvent {
        id: Some(crate::uuid::generate_v4()),
        kind: DaemonLogKind::Log,
        timestamp: chrono::Utc::now().to_rfc3339(),
        level: DaemonLogLevel::Error,
        target: Some("git_ai::daemon::memory_watchdog".to_string()),
        message: "daemon memory emergency threshold reached".to_string(),
        fields,
        repo_url: None,
        git_ai_version: None,
    };
    match upload_emergency_daemon_log(event, EMERGENCY_LOG_UPLOAD_TIMEOUT) {
        EmergencyLogUploadStatus::Completed => {}
        EmergencyLogUploadStatus::TimedOut => {
            eprintln!("[git-ai] emergency daemon log upload timed out; continuing shutdown");
        }
        EmergencyLogUploadStatus::ThreadUnavailable => {
            eprintln!("[git-ai] emergency daemon log upload could not start; continuing shutdown");
        }
    }
    let _ = io::stderr().flush();
}

fn watchdog_poll_interval() -> Duration {
    #[cfg(feature = "test-support")]
    if let Ok(raw) = std::env::var(TEST_POLL_INTERVAL_ENV)
        && let Ok(milliseconds) = raw.parse::<u64>()
        && milliseconds > 0
    {
        return Duration::from_millis(milliseconds);
    }

    WATCHDOG_POLL_INTERVAL
}

pub(super) fn decision_for_rss(
    rss_bytes: u64,
    thresholds: MemoryThresholds,
) -> MemoryWatchdogDecision {
    if rss_bytes >= thresholds.emergency_bytes {
        return MemoryWatchdogDecision::Abort;
    }
    MemoryWatchdogDecision::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: u64 = 1024 * 1024 * 1024;

    #[test]
    fn memory_limit_thresholds_use_eighty_five_percent_headroom() {
        let thresholds = MemoryThresholds::from_limit_bytes(LIMIT);

        assert_eq!(thresholds.emergency_bytes, 912_680_551);
        assert_eq!(thresholds.limit_bytes, LIMIT);
    }

    #[test]
    fn watchdog_aborts_at_the_emergency_threshold() {
        let thresholds = MemoryThresholds::from_limit_bytes(LIMIT);
        assert_eq!(
            decision_for_rss(thresholds.emergency_bytes - 1, thresholds),
            MemoryWatchdogDecision::Continue
        );
        assert_eq!(
            decision_for_rss(thresholds.emergency_bytes, thresholds),
            MemoryWatchdogDecision::Abort
        );
    }

    #[test]
    fn watchdog_aborts_at_the_hard_threshold() {
        let thresholds = MemoryThresholds::from_limit_bytes(LIMIT);
        assert_eq!(
            decision_for_rss(thresholds.emergency_bytes - 1, thresholds),
            MemoryWatchdogDecision::Continue
        );
        assert_eq!(
            decision_for_rss(thresholds.limit_bytes, thresholds),
            MemoryWatchdogDecision::Abort
        );
    }
}
