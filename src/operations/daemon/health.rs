//! Bounded, observational health snapshot for the daemon control API.

use super::{ActorDaemonCoordinator, FamilySequencerEntry};
use crate::operations::daemon::log_setup::now_unix_nanos;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(crate) const HEALTH_FAMILY_OBSERVATION_LIMIT: usize = 32;
pub(crate) const HEALTH_SEQUENCER_ENTRY_OBSERVATION_LIMIT: usize = 256;
pub(crate) const HEALTH_OPEN_ROOT_OBSERVATION_LIMIT: usize = 64;
// This matches the conservative daemon liveness window: long hooks are busy,
// while work with no progress for half an hour is actionable as stalled.
const SEQUENCER_STALL_THRESHOLD: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct HealthObservationLimits {
    families: usize,
    sequencer_entries: usize,
    open_roots: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct FamilyHealth {
    pub family_id: String,
    pub entries: usize,
    pub entries_pending_roots: usize,
    pub entries_commands: usize,
    pub entries_checkpoints: usize,
    pub entries_canceled: usize,
    pub oldest_entry_age_ms: u64,
    pub front_kind: Option<&'static str>,
    pub fenced: bool,
    pub inflight_effects: usize,
    pub side_effect_errors: usize,
    #[serde(skip)]
    key: String,
    #[serde(skip)]
    front_started_at_ns: Option<u128>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DaemonHealthSnapshot {
    pub uptime_seconds: u64,
    /// When true, numeric observations are lower bounds from the fixed-cap
    /// sample rather than invented complete totals.
    pub snapshot_partial: bool,
    pub observation_limits: HealthObservationLimits,
    pub checkpoints_outstanding: usize,
    pub checkpoints_unadmitted: usize,
    pub trace_payloads_queued: usize,
    pub trace_ingest_seq_enqueued: u64,
    pub trace_ingest_seq_processed: u64,
    pub trace_ingest_seq_lag: u64,
    pub trace_roots_open_mutating: usize,
    pub trace_root_oldest_open_age_ms: Option<u64>,
    pub trace_root_oldest_idle_ms: Option<u64>,
    pub trace_connections_unidentified: usize,
    pub sequencer_families: usize,
    pub sequencer_entries_total: usize,
    pub sequencer_entries_pending_roots: usize,
    pub sequencer_entries_commands: usize,
    pub sequencer_entries_checkpoints: usize,
    pub sequencer_entries_canceled: usize,
    pub sequencer_oldest_entry_age_ms: Option<u64>,
    pub sequencer_fenced_families: usize,
    pub sequencer_stall_threshold_ms: u64,
    pub sequencer_stalled: bool,
    pub effects_inflight_families: usize,
    pub effects_inflight_total: usize,
    pub side_effect_error_families: usize,
    pub side_effect_errors_total: usize,
    pub trace_payloads_dropped_queue_full: u64,
    pub trace_ingest_worker_disconnects: u64,
    pub checkpoint_requests_rejected: u64,
    pub families: Vec<FamilyHealth>,
}

#[derive(Debug, Clone)]
struct OpenRootSample {
    family: Option<String>,
    started_at_ns: Option<u128>,
    last_activity_ns: Option<u128>,
}

impl DaemonHealthSnapshot {
    pub(crate) fn capture(coordinator: &ActorDaemonCoordinator) -> Self {
        let now_ns = now_unix_nanos();
        let mut snapshot_partial = false;
        let mut families = BTreeMap::new();
        let mut entries_observed = 0usize;
        let mut entries_pending_roots = 0usize;
        let mut entries_commands = 0usize;
        let mut entries_checkpoints = 0usize;
        let mut entries_canceled = 0usize;
        let mut oldest_entry_age_ms = None;

        match coordinator.family_sequencers_by_family.try_lock() {
            Ok(sequencers) => {
                if sequencers.len() > HEALTH_FAMILY_OBSERVATION_LIMIT {
                    snapshot_partial = true;
                }
                for (key, state) in sequencers.iter().take(HEALTH_FAMILY_OBSERVATION_LIMIT) {
                    if state.entries.is_empty() {
                        continue;
                    }
                    let remaining =
                        HEALTH_SEQUENCER_ENTRY_OBSERVATION_LIMIT.saturating_sub(entries_observed);
                    if state.entries.len() > remaining {
                        snapshot_partial = true;
                    }
                    if remaining == 0 {
                        break;
                    }
                    let Some(health) = family_health(&mut families, key, &mut snapshot_partial)
                    else {
                        continue;
                    };
                    if let Some((order, entry)) = state.entries.first_key_value() {
                        health.front_kind = Some(entry_kind(entry));
                        health.front_started_at_ns = Some(order.started_at_ns);
                    }
                    for (order, entry) in state.entries.iter().take(remaining) {
                        let age_ms = age_ms(now_ns, order.started_at_ns);
                        health.entries = health.entries.saturating_add(1);
                        health.oldest_entry_age_ms = health.oldest_entry_age_ms.max(age_ms);
                        oldest_entry_age_ms = Some(
                            oldest_entry_age_ms.map_or(age_ms, |oldest: u64| oldest.max(age_ms)),
                        );
                        entries_observed = entries_observed.saturating_add(1);
                        match entry {
                            FamilySequencerEntry::PendingRoot => {
                                health.entries_pending_roots += 1;
                                entries_pending_roots += 1;
                            }
                            FamilySequencerEntry::ReadyCommand(_) => {
                                health.entries_commands += 1;
                                entries_commands += 1;
                            }
                            FamilySequencerEntry::Checkpoint { .. } => {
                                health.entries_checkpoints += 1;
                                entries_checkpoints += 1;
                            }
                            FamilySequencerEntry::Canceled => {
                                health.entries_canceled += 1;
                                entries_canceled += 1;
                            }
                        }
                    }
                }
            }
            Err(_) => snapshot_partial = true,
        }

        let mut open_roots = Vec::new();
        let mut trace_connections_unidentified = 0usize;
        let mut oldest_open_age_ms = None;
        let mut oldest_root_idle_ms = None;
        match coordinator.trace_ingress_state.try_lock() {
            Ok(ingress) => {
                trace_connections_unidentified = ingress.unidentified_open_connections;
                if ingress.root_open_connections.len() > HEALTH_OPEN_ROOT_OBSERVATION_LIMIT {
                    snapshot_partial = true;
                }
                for (root, count) in ingress
                    .root_open_connections
                    .iter()
                    .take(HEALTH_OPEN_ROOT_OBSERVATION_LIMIT)
                {
                    if *count == 0
                        || ingress.root_definitely_read_only.contains(root)
                        || !ingress.root_mutating.get(root).copied().unwrap_or(true)
                    {
                        continue;
                    }
                    let started_at_ns = ingress.root_started_at_ns.get(root).copied();
                    if let Some(started_at_ns) = started_at_ns {
                        let age_ms = age_ms(now_ns, started_at_ns);
                        oldest_open_age_ms = Some(
                            oldest_open_age_ms.map_or(age_ms, |oldest: u64| oldest.max(age_ms)),
                        );
                    }
                    let last_activity_ns = ingress
                        .root_last_activity_ns
                        .get(root)
                        .copied()
                        .map(u128::from);
                    if let Some(last_activity_ns) = last_activity_ns {
                        let idle_ms = age_ms(now_ns, last_activity_ns);
                        oldest_root_idle_ms = Some(
                            oldest_root_idle_ms.map_or(idle_ms, |oldest: u64| oldest.max(idle_ms)),
                        );
                    }
                    open_roots.push(OpenRootSample {
                        family: ingress.root_families.get(root).cloned(),
                        started_at_ns,
                        last_activity_ns,
                    });
                }
            }
            Err(_) => snapshot_partial = true,
        }

        let (effects_inflight_families, effects_inflight_total) = match coordinator
            .inflight_effects_by_family
            .try_lock()
        {
            Ok(effects) => {
                if effects.len() > HEALTH_FAMILY_OBSERVATION_LIMIT {
                    snapshot_partial = true;
                }
                let mut family_count = 0usize;
                let mut total = 0usize;
                for (key, count) in effects.iter().take(HEALTH_FAMILY_OBSERVATION_LIMIT) {
                    if *count == 0 {
                        continue;
                    }
                    family_count += 1;
                    total = total.saturating_add(*count);
                    if let Some(health) = family_health(&mut families, key, &mut snapshot_partial) {
                        health.inflight_effects = *count;
                    }
                }
                (family_count, total)
            }
            Err(_) => {
                snapshot_partial = true;
                (0, 0)
            }
        };

        let (side_effect_error_families, side_effect_errors_total) = match coordinator
            .side_effect_errors_by_family
            .try_lock()
        {
            Ok(errors) => {
                if errors.len() > HEALTH_FAMILY_OBSERVATION_LIMIT {
                    snapshot_partial = true;
                }
                let mut family_count = 0usize;
                let mut total = 0usize;
                for (key, family_errors) in errors.iter().take(HEALTH_FAMILY_OBSERVATION_LIMIT) {
                    if family_errors.is_empty() {
                        continue;
                    }
                    family_count += 1;
                    total = total.saturating_add(family_errors.len());
                    if let Some(health) = family_health(&mut families, key, &mut snapshot_partial) {
                        health.side_effect_errors = family_errors.len();
                    }
                }
                (family_count, total)
            }
            Err(_) => {
                snapshot_partial = true;
                (0, 0)
            }
        };

        let mut fenced_families = 0usize;
        let mut sequencer_stalled = false;
        for health in families.values_mut().filter(|health| health.entries > 0) {
            let mut has_blocking_root = false;
            let mut has_idle_blocking_root = false;
            for root in &open_roots {
                if !root_blocks_family(root, health) {
                    continue;
                }
                has_blocking_root = true;
                has_idle_blocking_root |= root.last_activity_ns.is_some_and(|last_activity_ns| {
                    Duration::from_millis(age_ms(now_ns, last_activity_ns))
                        >= SEQUENCER_STALL_THRESHOLD
                });
            }
            let pending_root = health.front_kind == Some("pending_root");
            health.fenced = pending_root || has_blocking_root;
            if health.fenced {
                fenced_families += 1;
            }
            if health.inflight_effects == 0
                && Duration::from_millis(health.oldest_entry_age_ms) >= SEQUENCER_STALL_THRESHOLD
                && (has_idle_blocking_root
                    || !health.fenced
                    || (pending_root && !has_blocking_root && !snapshot_partial))
            {
                sequencer_stalled = true;
            }
        }

        let mut families = families.into_values().collect::<Vec<_>>();
        families.sort_by(|left, right| {
            right
                .oldest_entry_age_ms
                .cmp(&left.oldest_entry_age_ms)
                .then_with(|| left.key.cmp(&right.key))
        });
        let trace_ingest_seq_enqueued =
            coordinator.next_trace_ingest_seq.load(Ordering::Acquire) as u64;
        let trace_ingest_seq_processed = coordinator
            .processed_trace_ingest_seq
            .load(Ordering::Acquire) as u64;

        Self {
            uptime_seconds: coordinator.started_at.elapsed().as_secs(),
            snapshot_partial,
            observation_limits: HealthObservationLimits {
                families: HEALTH_FAMILY_OBSERVATION_LIMIT,
                sequencer_entries: HEALTH_SEQUENCER_ENTRY_OBSERVATION_LIMIT,
                open_roots: HEALTH_OPEN_ROOT_OBSERVATION_LIMIT,
            },
            checkpoints_outstanding: coordinator
                .checkpoint_requests_outstanding
                .load(Ordering::Acquire),
            checkpoints_unadmitted: coordinator
                .checkpoint_requests_unadmitted
                .load(Ordering::Acquire),
            trace_payloads_queued: coordinator.queued_trace_payloads.load(Ordering::Acquire),
            trace_ingest_seq_enqueued,
            trace_ingest_seq_processed,
            trace_ingest_seq_lag: trace_ingest_seq_enqueued
                .saturating_sub(trace_ingest_seq_processed),
            trace_roots_open_mutating: open_roots.len(),
            trace_root_oldest_open_age_ms: oldest_open_age_ms,
            trace_root_oldest_idle_ms: oldest_root_idle_ms,
            trace_connections_unidentified,
            sequencer_families: families.iter().filter(|health| health.entries > 0).count(),
            sequencer_entries_total: entries_observed,
            sequencer_entries_pending_roots: entries_pending_roots,
            sequencer_entries_commands: entries_commands,
            sequencer_entries_checkpoints: entries_checkpoints,
            sequencer_entries_canceled: entries_canceled,
            sequencer_oldest_entry_age_ms: oldest_entry_age_ms,
            sequencer_fenced_families: fenced_families,
            sequencer_stall_threshold_ms: SEQUENCER_STALL_THRESHOLD.as_millis() as u64,
            sequencer_stalled,
            effects_inflight_families,
            effects_inflight_total,
            side_effect_error_families,
            side_effect_errors_total,
            trace_payloads_dropped_queue_full: coordinator
                .trace_payloads_dropped_queue_full
                .load(Ordering::Relaxed),
            trace_ingest_worker_disconnects: coordinator
                .trace_ingest_worker_disconnects
                .load(Ordering::Relaxed),
            checkpoint_requests_rejected: coordinator
                .checkpoint_requests_rejected
                .load(Ordering::Relaxed),
            families,
        }
    }
}

fn family_health<'a>(
    families: &'a mut BTreeMap<String, FamilyHealth>,
    key: &str,
    snapshot_partial: &mut bool,
) -> Option<&'a mut FamilyHealth> {
    if !families.contains_key(key) && families.len() >= HEALTH_FAMILY_OBSERVATION_LIMIT {
        *snapshot_partial = true;
        return None;
    }
    Some(
        families
            .entry(key.to_string())
            .or_insert_with(|| FamilyHealth {
                family_id: family_id(key),
                key: key.to_string(),
                ..FamilyHealth::default()
            }),
    )
}

fn family_id(key: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(key.as_bytes());
    let mut id = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        id.push(HEX[(byte >> 4) as usize] as char);
        id.push(HEX[(byte & 0x0f) as usize] as char);
    }
    id
}

fn root_blocks_family(root: &OpenRootSample, family: &FamilyHealth) -> bool {
    root.family
        .as_deref()
        .is_none_or(|root_family| root_family == family.key)
        && root.started_at_ns.is_none_or(|started_at_ns| {
            family
                .front_started_at_ns
                .is_some_and(|front| started_at_ns <= front)
        })
}

fn entry_kind(entry: &FamilySequencerEntry) -> &'static str {
    match entry {
        FamilySequencerEntry::PendingRoot => "pending_root",
        FamilySequencerEntry::ReadyCommand(_) => "command",
        FamilySequencerEntry::Checkpoint { .. } => "checkpoint",
        FamilySequencerEntry::Canceled => "canceled",
    }
}

fn age_ms(now_ns: u128, started_at_ns: u128) -> u64 {
    u64::try_from(now_ns.saturating_sub(started_at_ns) / 1_000_000).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::daemon::FamilySequencerState;

    #[tokio::test]
    async fn daemon_health_contended_state_returns_a_partial_snapshot_promptly() {
        let coordinator = ActorDaemonCoordinator::new();
        let _held = coordinator.family_sequencers_by_family.lock().unwrap();
        let started = std::time::Instant::now();

        let snapshot = DaemonHealthSnapshot::capture(&coordinator);

        assert!(snapshot.snapshot_partial);
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn daemon_health_snapshot_stops_at_its_family_cap() {
        let coordinator = ActorDaemonCoordinator::new();
        {
            let mut sequencers = coordinator.family_sequencers_by_family.lock().unwrap();
            for index in 0..=HEALTH_FAMILY_OBSERVATION_LIMIT {
                let mut state = FamilySequencerState::new();
                state.insert_entry(now_unix_nanos(), FamilySequencerEntry::Canceled);
                sequencers.insert(format!("family-{index}"), state);
            }
        }

        let snapshot = DaemonHealthSnapshot::capture(&coordinator);

        assert!(snapshot.snapshot_partial);
        assert_eq!(snapshot.families.len(), HEALTH_FAMILY_OBSERVATION_LIMIT);
        assert_eq!(
            snapshot.sequencer_entries_total,
            HEALTH_FAMILY_OBSERVATION_LIMIT
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(
            !json.contains("family-"),
            "daemon-wide health must not expose repository paths: {json}"
        );
        assert!(
            snapshot
                .families
                .iter()
                .all(|family| family.family_id.len() == 16)
        );
    }

    #[tokio::test]
    async fn daemon_health_reports_fail_closed_ingest_losses() {
        let queue_full = ActorDaemonCoordinator::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        queue_full.trace_ingest_tx.set(tx).unwrap();
        queue_full
            .enqueue_trace_payload(serde_json::json!({ "event": "version" }))
            .unwrap();
        assert!(
            queue_full
                .enqueue_trace_payload(serde_json::json!({ "event": "version" }))
                .is_err()
        );
        let full_snapshot = DaemonHealthSnapshot::capture(&queue_full);
        assert_eq!(full_snapshot.trace_payloads_dropped_queue_full, 1);

        let disconnected = ActorDaemonCoordinator::new();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(rx);
        disconnected.trace_ingest_tx.set(tx).unwrap();
        assert!(
            disconnected
                .enqueue_trace_payload(serde_json::json!({ "event": "version" }))
                .is_err()
        );
        let disconnected_snapshot = DaemonHealthSnapshot::capture(&disconnected);
        assert_eq!(disconnected_snapshot.trace_ingest_worker_disconnects, 1);
    }

    #[tokio::test]
    async fn daemon_health_only_stalls_a_fence_after_root_activity_stops() {
        let coordinator = ActorDaemonCoordinator::new();
        let now_ns = now_unix_nanos();
        let stale_ns = now_ns.saturating_sub(SEQUENCER_STALL_THRESHOLD.as_nanos() + 1_000_000);
        let now_ns_u64 = u64::try_from(now_ns).unwrap_or(u64::MAX);
        let stale_ns_u64 = u64::try_from(stale_ns).unwrap_or(u64::MAX);
        {
            let mut sequencers = coordinator.family_sequencers_by_family.lock().unwrap();
            let mut state = FamilySequencerState::new();
            state.insert_entry(stale_ns, FamilySequencerEntry::PendingRoot);
            sequencers.insert("family".to_string(), state);
        }
        {
            let mut ingress = coordinator.trace_ingress_state.lock().unwrap();
            ingress.root_open_connections.insert("root".to_string(), 1);
            ingress.root_mutating.insert("root".to_string(), true);
            ingress
                .root_families
                .insert("root".to_string(), "family".to_string());
            ingress
                .root_started_at_ns
                .insert("root".to_string(), stale_ns);
            ingress
                .root_last_activity_ns
                .insert("root".to_string(), now_ns_u64);
        }

        assert!(!DaemonHealthSnapshot::capture(&coordinator).sequencer_stalled);

        coordinator
            .trace_ingress_state
            .lock()
            .unwrap()
            .root_last_activity_ns
            .insert("root".to_string(), stale_ns_u64);
        assert!(DaemonHealthSnapshot::capture(&coordinator).sequencer_stalled);
    }
}
