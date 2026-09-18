use super::actor_types::ActorDaemonCoordinator;
use crate::error::GitAiError;
use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::{Duration, Instant};

const MAX_FAMILIES: usize = 1024;
const SUMMARY_INTERVAL: Duration = Duration::from_secs(30 * 60);
const IDLE_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const GC_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Error,
    Debug,
    Summary(u64),
}

struct FailureRun {
    fingerprint: u64,
    consecutive: u64,
    suppressed: u64,
    last_summary: Option<Instant>,
    last_seen: Instant,
}

#[derive(Default)]
pub(crate) struct ErrorLogPolicy {
    families: HashMap<String, FailureRun>,
    next_gc: Option<Instant>,
}

impl ErrorLogPolicy {
    fn success(&mut self, family: &str) {
        self.families.remove(family);
    }

    fn failure(&mut self, family: &str, phase: &str, error: &GitAiError, now: Instant) -> Decision {
        // This bounded scan runs only on the side-effect error path, never ingestion.
        if self.next_gc.is_none_or(|deadline| now >= deadline) {
            self.families
                .retain(|_, run| now.saturating_duration_since(run.last_seen) < IDLE_TIMEOUT);
            self.next_gc = Some(now + GC_INTERVAL);
        }
        if !self.families.contains_key(family)
            && self.families.len() >= MAX_FAMILIES
            && let Some(oldest) = self
                .families
                .iter()
                .min_by_key(|(_, run)| run.last_seen)
                .map(|(family, _)| family.clone())
        {
            self.families.remove(&oldest);
        }
        let fingerprint = fingerprint(phase, error);
        let fresh = || FailureRun {
            fingerprint,
            consecutive: 0,
            suppressed: 0,
            last_summary: None,
            last_seen: now,
        };
        let run = self
            .families
            .entry(family.to_string())
            .or_insert_with(fresh);
        if run.fingerprint != fingerprint
            || now.saturating_duration_since(run.last_seen) >= IDLE_TIMEOUT
        {
            *run = fresh();
        }
        run.last_seen = now;
        run.consecutive = run.consecutive.saturating_add(1);
        if run.consecutive <= 3 {
            return Decision::Error;
        }
        run.suppressed = run.suppressed.saturating_add(1);
        if run
            .last_summary
            .is_none_or(|last| now.saturating_duration_since(last) >= SUMMARY_INTERVAL)
        {
            run.last_summary = Some(now);
            return Decision::Summary(std::mem::take(&mut run.suppressed));
        }
        Decision::Debug
    }
}

fn fingerprint(phase: &str, error: &GitAiError) -> u64 {
    let mut hash = DefaultHasher::new();
    phase.hash(&mut hash);
    std::mem::discriminant(error).hash(&mut hash);
    match error {
        GitAiError::GitCliError { code, stderr, .. } => {
            code.hash(&mut hash);
            // Full OIDs vary between invocations; short hex/numeric runs can be
            // meaningful error codes, limits, or paths and must stay distinct.
            let bytes = stderr.as_bytes();
            let mut start = 0;
            while start < bytes.len() {
                let hex = bytes[start].is_ascii_hexdigit();
                let end = start
                    + bytes[start..]
                        .iter()
                        .take_while(|byte| byte.is_ascii_hexdigit() == hex)
                        .count();
                if hex && matches!(end - start, 40 | 64) {
                    b"<oid>".hash(&mut hash);
                } else {
                    bytes[start..end].hash(&mut hash);
                }
                start = end;
            }
        }
        _ => error.to_string().hash(&mut hash),
    }
    hash.finish()
}

pub(crate) fn is_discovery_miss(error: &GitAiError) -> bool {
    matches!(error, GitAiError::Generic(message) if message.starts_with("No git repository found for path without exec: "))
}

fn definitely_missing(metadata: std::io::Result<std::fs::Metadata>) -> bool {
    matches!(metadata, Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

fn expected_condition(family: &str, error: &GitAiError) -> bool {
    if is_discovery_miss(error) {
        return true;
    }
    let GitAiError::GitCliError { stderr, .. } = error else {
        return false;
    };
    let missing_repo = stderr.starts_with("fatal: cannot change to ")
        || stderr.starts_with("fatal: not a git repository")
        || stderr.starts_with("fatal: failed to stat ");
    missing_repo && definitely_missing(std::fs::metadata(Path::new(family)))
}

impl ActorDaemonCoordinator {
    pub(crate) fn record_and_log_side_effect_result<T>(
        &self,
        family: &str,
        order: u64,
        phase: &'static str,
        message: &'static str,
        result: &Result<T, GitAiError>,
    ) {
        let Err(error) = result else {
            if let Ok(mut policy) = self.error_log_policy.lock() {
                policy.success(family);
            }
            return;
        };
        // Status and completion records retain every error even when logs are quiet.
        let _ = self.record_side_effect_error(family, order, error);
        if expected_condition(family, error) {
            if let Ok(mut policy) = self.error_log_policy.lock() {
                policy.success(family);
            }
            tracing::debug!(%error, %family, order, phase, reason = "repository_unavailable", "{message}");
            return;
        }
        // Logging failures must fail loud, without changing the operation result.
        let decision = self
            .error_log_policy
            .lock()
            .map(|mut policy| policy.failure(family, phase, error, Instant::now()))
            .unwrap_or(Decision::Error);
        match decision {
            Decision::Error => tracing::error!(%error, %family, order, phase, "{message}"),
            Decision::Debug => tracing::debug!(%error, %family, order, phase, "{message}"),
            Decision::Summary(suppressed) => {
                tracing::debug!(%error, %family, order, phase, "{message}");
                tracing::warn!(%error, %family, order, phase, suppressed, "repeated daemon failures suppressed");
            }
        }
    }
}

#[cfg(test)]
mod tests;
