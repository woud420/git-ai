//! Daemon-side transcript worker for sweep-based transcript discovery.
//!
//! Runs inside the daemon process with two event sources:
//! 1. **Checkpoint notifications** (Immediate priority, <100ms) - fired when `git-ai checkpoint` is called
//! 2. **Periodic sweeps** (Low priority, every 30min) - agent-specific discovery of all sessions

use crate::config;
use crate::model::checkpoint_request::StreamFormat as CheckpointStreamFormat;
use crate::model::repository::streams_db::StreamsDatabase;
use crate::operations::daemon::telemetry_worker::DaemonTelemetryWorkerHandle;
pub use crate::operations::streams::timestamp::parse_event_timestamp as extract_event_timestamp;
use std::collections::{BinaryHeap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::Notify;
use tokio::time::{Duration, interval};

mod checkpoint_notification;
#[cfg(test)]
mod checkpoint_notification_tests;
mod processing;
mod sweep;

const TRIGGERED_SWEEP_COOLDOWN: Duration = Duration::from_secs(30);

/// Collection is opt-in per repository: a session's transcript may only be
/// processed when its working directory resolves to a repository allowed by
/// `allowed_repositories`. Sessions with no resolvable git repository are
/// skipped as well (fail closed).
pub(crate) fn transcript_collection_allowed(work_dir: Option<&Path>) -> bool {
    let Some(work_dir) = work_dir else {
        return false;
    };
    match crate::operations::git::repository::discover_repository_in_path_no_git_exec(work_dir) {
        Ok(repo) => repo.is_collection_allowed(&crate::config::Config::fresh()),
        Err(_) => false,
    }
}

/// Priority levels for processing tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(serde::Serialize, serde::Deserialize))]
pub(super) enum Priority {
    Low = 2, // Sweep-discovered sessions
    Immediate = 0, // Checkpoint-triggered, process first
             // REMOVED: High = 1 (was polling)
}

/// Task to process a session's transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Serialize, serde::Deserialize))]
pub(super) struct ProcessingTask {
    pub(super) priority: Priority,
    pub(super) session_id: String,
    pub(super) stream_kind: String,
    pub(super) tool: String,
    pub(super) trace_id: Option<String>,
    pub(super) tool_use_id: Option<String>,
    pub(super) canonical_path: PathBuf,
    pub(super) repo_work_dir: Option<PathBuf>,
    pub(super) retry_count: u32,
    #[cfg_attr(test, serde(skip))]
    pub(super) next_retry_at: Option<std::time::Instant>,
}

impl PartialOrd for ProcessingTask {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ProcessingTask {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher priority first (Immediate=0 < High=1 < Low=2)
        // Reverse comparison so smaller numeric value = higher priority = popped first from max-heap
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| self.session_id.cmp(&other.session_id))
    }
}

/// Source of an explicit transcript sweep request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepTrigger {
    PostCommit,
    PostPush,
}

impl std::fmt::Display for SweepTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PostCommit => f.write_str("post_commit"),
            Self::PostPush => f.write_str("post_push"),
        }
    }
}

struct SweepRequest {
    trigger: SweepTrigger,
    priority: Priority,
    completion: Option<std::sync::mpsc::Sender<Result<(), String>>>,
}

struct DrainRequest {
    completion: tokio::sync::oneshot::Sender<()>,
}

impl SweepRequest {
    fn normal(trigger: SweepTrigger) -> Self {
        Self {
            trigger,
            priority: Priority::Low,
            completion: None,
        }
    }
}

#[derive(Clone)]
struct SweepTriggerGate {
    last_triggered_at: Arc<Mutex<Option<Instant>>>,
}

impl SweepTriggerGate {
    fn new() -> Self {
        Self {
            last_triggered_at: Arc::new(Mutex::new(None)),
        }
    }

    fn try_trigger_at(
        &self,
        now: Instant,
        source: &str,
        trigger_action: impl FnOnce() -> bool,
    ) -> bool {
        let Ok(mut last_triggered_at) = self.last_triggered_at.lock() else {
            tracing::warn!("failed to lock transcript sweep trigger cooldown");
            return false;
        };

        if let Some(last) = *last_triggered_at {
            let elapsed = now.checked_duration_since(last).unwrap_or_default();
            if elapsed < TRIGGERED_SWEEP_COOLDOWN {
                tracing::debug!(
                    source,
                    elapsed_ms = elapsed.as_millis() as u64,
                    "transcript sweep trigger suppressed by cooldown"
                );
                return false;
            }
        }

        if !trigger_action() {
            return false;
        }

        *last_triggered_at = Some(now);
        true
    }

    fn try_mark_sweep_at(&self, now: Instant, source: &str) -> bool {
        self.try_trigger_at(now, source, || true)
    }

    fn force_trigger_at(&self, now: Instant, trigger_action: impl FnOnce() -> bool) -> bool {
        let Ok(mut last_triggered_at) = self.last_triggered_at.lock() else {
            tracing::warn!("failed to lock transcript sweep trigger cooldown");
            return false;
        };

        if !trigger_action() {
            return false;
        }

        *last_triggered_at = Some(now);
        true
    }
}

/// Handle for sending checkpoint notifications and sweep requests to the worker.
#[derive(Clone)]
pub struct StreamWorkerHandle {
    checkpoint_tx: tokio::sync::mpsc::UnboundedSender<CheckpointNotification>,
    sweep_tx: tokio::sync::mpsc::UnboundedSender<SweepRequest>,
    drain_tx: tokio::sync::mpsc::UnboundedSender<DrainRequest>,
    sweep_trigger_gate: SweepTriggerGate,
}

impl StreamWorkerHandle {
    /// Notify the worker that a checkpoint was recorded.
    #[allow(clippy::too_many_arguments)]
    pub fn notify_checkpoint(
        &self,
        session_id: String,
        tool: String,
        trace_id: String,
        tool_use_id: Option<String>,
        stream_path: PathBuf,
        stream_format: CheckpointStreamFormat,
        repo_work_dir: Option<PathBuf>,
        external_session_id: String,
        external_parent_session_id: Option<String>,
    ) {
        let notification = CheckpointNotification {
            session_id,
            tool,
            trace_id,
            tool_use_id,
            stream_path,
            stream_format,
            repo_work_dir,
            external_session_id,
            external_parent_session_id,
        };
        let _ = self.checkpoint_tx.send(notification);
    }

    /// Request a full sweep unless another sweep was triggered recently.
    ///
    /// Returns true when a request was sent to the worker, false when it was
    /// suppressed by the cooldown or the worker has already stopped.
    pub fn trigger_sweep(&self, trigger: SweepTrigger) -> bool {
        self.trigger_sweep_at(trigger, Instant::now())
    }

    /// Request a sweep for commit-time attribution recovery.
    ///
    /// This bypasses the user-facing cooldown because the caller performs its own
    /// short bounded wait for a repo-linked metric row, but still marks the gate
    /// so the regular post-commit sweep for the same command is suppressed.
    pub fn trigger_sweep_for_recovery(
        &self,
        trigger: SweepTrigger,
    ) -> Option<std::sync::mpsc::Receiver<Result<(), String>>> {
        let (completion_tx, completion_rx) = std::sync::mpsc::channel();
        let mut completion_tx = Some(completion_tx);
        let sent = self
            .sweep_trigger_gate
            .force_trigger_at(Instant::now(), || {
                self.sweep_tx
                    .send(SweepRequest {
                        trigger,
                        priority: Priority::Immediate,
                        completion: completion_tx.take(),
                    })
                    .is_ok()
            });
        sent.then_some(completion_rx)
    }

    fn trigger_sweep_at(&self, trigger: SweepTrigger, now: Instant) -> bool {
        let source = trigger.to_string();
        self.sweep_trigger_gate.try_trigger_at(now, &source, || {
            self.sweep_tx.send(SweepRequest::normal(trigger)).is_ok()
        })
    }

    /// Request that the worker drain all immediate processing tasks.
    ///
    /// Returns after the worker has finished processing all currently-enqueued
    /// immediate-priority tasks and any that are already in flight.
    pub async fn drain(&self) -> Result<(), String> {
        let (completion_tx, completion_rx) = tokio::sync::oneshot::channel();
        self.drain_tx
            .send(DrainRequest {
                completion: completion_tx,
            })
            .map_err(|_| "stream worker has stopped".to_string())?;
        completion_rx
            .await
            .map_err(|_| "stream worker drain was cancelled".to_string())
    }

    #[cfg(test)]
    fn for_test_sweep_triggers(sweep_tx: tokio::sync::mpsc::UnboundedSender<SweepRequest>) -> Self {
        let (checkpoint_tx, _checkpoint_rx) = tokio::sync::mpsc::unbounded_channel();
        let (drain_tx, _drain_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            checkpoint_tx,
            sweep_tx,
            drain_tx,
            sweep_trigger_gate: SweepTriggerGate::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct CheckpointNotification {
    session_id: String,
    tool: String,
    trace_id: String,
    tool_use_id: Option<String>,
    stream_path: PathBuf,
    stream_format: CheckpointStreamFormat,
    repo_work_dir: Option<PathBuf>,
    external_session_id: String,
    external_parent_session_id: Option<String>,
}

/// Worker that processes transcript changes.
struct StreamWorker {
    streams_db: Arc<StreamsDatabase>,
    sweep_coordinator: crate::operations::daemon::sweep_coordinator::SweepCoordinator, // NEW
    priority_queue: BinaryHeap<ProcessingTask>,
    delayed_tasks: Vec<ProcessingTask>,
    in_flight: HashSet<(PathBuf, String)>,
    telemetry_handle: DaemonTelemetryWorkerHandle,
    shutdown_notify: Arc<Notify>,
    shutdown_flag: Arc<AtomicBool>,
    checkpoint_rx: tokio::sync::mpsc::UnboundedReceiver<CheckpointNotification>,
    sweep_rx: tokio::sync::mpsc::UnboundedReceiver<SweepRequest>,
    drain_rx: tokio::sync::mpsc::UnboundedReceiver<DrainRequest>,
    sweep_trigger_gate: SweepTriggerGate,
}

impl StreamWorker {
    /// Create a new transcript worker.
    #[allow(clippy::too_many_arguments)]
    fn new(
        streams_db: Arc<StreamsDatabase>,
        telemetry_handle: DaemonTelemetryWorkerHandle,
        shutdown_notify: Arc<Notify>,
        shutdown_flag: Arc<AtomicBool>,
        checkpoint_rx: tokio::sync::mpsc::UnboundedReceiver<CheckpointNotification>,
        sweep_rx: tokio::sync::mpsc::UnboundedReceiver<SweepRequest>,
        drain_rx: tokio::sync::mpsc::UnboundedReceiver<DrainRequest>,
        sweep_trigger_gate: SweepTriggerGate,
    ) -> Self {
        let sweep_coordinator =
            crate::operations::daemon::sweep_coordinator::SweepCoordinator::new(streams_db.clone());

        Self {
            streams_db,
            sweep_coordinator, // NEW
            priority_queue: BinaryHeap::new(),
            delayed_tasks: Vec::new(),
            in_flight: HashSet::new(),
            telemetry_handle,
            shutdown_notify,
            shutdown_flag,
            checkpoint_rx,
            sweep_rx,
            drain_rx,
            sweep_trigger_gate,
        }
    }

    /// Main processing loop.
    async fn run(mut self) {
        tracing::info!("transcript worker started");

        let sweep_enabled = config::Config::get().get_feature_flags().transcript_sweep;

        let mut sweep_ticker = interval(Duration::from_secs(30 * 60)); // NEW: 30 minutes

        // Skip the first immediate tick
        sweep_ticker.tick().await;

        // Run initial sweep on startup
        if sweep_enabled
            && self
                .sweep_trigger_gate
                .try_mark_sweep_at(Instant::now(), "initial")
            && let Err(e) = self.run_sweep(Priority::Low).await
        {
            tracing::error!(error = %e, "initial sweep failed");
        }

        loop {
            self.promote_ready_delayed_tasks(Instant::now());
            let has_ready_task = !self.priority_queue.is_empty();
            let next_retry_at = if has_ready_task {
                None
            } else {
                self.next_delayed_task_at()
            };
            let retry_sleep = async {
                if let Some(at) = next_retry_at {
                    tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await;
                } else {
                    std::future::pending::<()>().await;
                }
            };

            tokio::select! {
                _ = self.shutdown_notify.notified() => {
                    tracing::info!("transcript worker received shutdown signal");
                    self.drain_immediate_tasks().await;
                    self.shutdown_flag.store(true, Ordering::Relaxed);
                    break;
                }
                _ = async {}, if has_ready_task => {
                    self.process_next_task().await;
                }
                _ = retry_sleep => {}
                _ = sweep_ticker.tick() => {  // NEW: sweep ticker
                    if sweep_enabled
                        && self
                            .sweep_trigger_gate
                            .try_mark_sweep_at(Instant::now(), "periodic")
                        && let Err(e) = self.run_sweep(Priority::Low).await
                    {
                        tracing::error!(error = %e, "sweep failed");
                    }
                }
                Some(notification) = self.checkpoint_rx.recv() => {
                    self.handle_checkpoint_notification(notification).await;
                }
                Some(request) = self.sweep_rx.recv() => {
                    self.handle_sweep_request(request, sweep_enabled).await;
                }
                Some(request) = self.drain_rx.recv() => {
                    self.handle_drain_request(request, sweep_enabled).await;
                }
            }
        }

        tracing::info!("transcript worker shutdown complete");
    }

    fn promote_ready_delayed_tasks(&mut self, now: std::time::Instant) {
        let mut i = 0;
        while i < self.delayed_tasks.len() {
            if self.delayed_tasks[i].next_retry_at.is_none_or(|t| now >= t) {
                let task = self.delayed_tasks.swap_remove(i);
                self.priority_queue.push(task);
            } else {
                i += 1;
            }
        }
    }

    async fn handle_sweep_request(&mut self, request: SweepRequest, sweep_enabled: bool) {
        if sweep_enabled {
            tracing::info!(trigger = %request.trigger, "triggered transcript sweep requested");
            let result = self.run_sweep(request.priority).await;
            if let Err(e) = &result {
                tracing::error!(trigger = %request.trigger, error = %e, "triggered sweep failed");
            }
            if let Some(completion) = request.completion {
                let _ = completion.send(result.map(|_| ()));
            }
        } else {
            tracing::debug!(
                trigger = %request.trigger,
                "triggered transcript sweep skipped because transcript_sweep is disabled"
            );
            if let Some(completion) = request.completion {
                let _ = completion.send(Err("transcript_sweep feature is disabled".to_string()));
            }
        }
    }

    async fn handle_drain_request(&mut self, request: DrainRequest, sweep_enabled: bool) {
        // Checkpoint and sweep requests use separate channels from the drain
        // barrier, so consume ingress that was already queued before the barrier.
        while let Ok(notification) = self.checkpoint_rx.try_recv() {
            self.handle_checkpoint_notification(notification).await;
        }
        while let Ok(sweep_request) = self.sweep_rx.try_recv() {
            self.handle_sweep_request(sweep_request, sweep_enabled)
                .await;
        }

        // Process immediate priority tasks that are already queued.
        self.drain_immediate_tasks().await;

        // Continue processing newly promoted tasks until there is no ready
        // work and no in-flight processing.
        while !self.priority_queue.is_empty() || !self.in_flight.is_empty() {
            self.process_next_task().await;
        }

        let _ = request.completion.send(());
    }

    fn next_delayed_task_at(&self) -> Option<std::time::Instant> {
        self.delayed_tasks
            .iter()
            .filter_map(|task| task.next_retry_at)
            .min()
    }
}

/// Spawn the transcript worker.
pub fn spawn_stream_worker(
    streams_db: Arc<StreamsDatabase>,
    telemetry_handle: DaemonTelemetryWorkerHandle,
    shutdown_notify: Arc<Notify>,
) -> StreamWorkerHandle {
    let (checkpoint_tx, checkpoint_rx) = tokio::sync::mpsc::unbounded_channel();
    let (sweep_tx, sweep_rx) = tokio::sync::mpsc::unbounded_channel();
    let (drain_tx, drain_rx) = tokio::sync::mpsc::unbounded_channel();
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let sweep_trigger_gate = SweepTriggerGate::new();

    let worker = StreamWorker::new(
        streams_db,
        telemetry_handle,
        shutdown_notify,
        shutdown_flag,
        checkpoint_rx,
        sweep_rx,
        drain_rx,
        sweep_trigger_gate.clone(),
    );

    tokio::spawn(async move {
        worker.run().await;
    });

    StreamWorkerHandle {
        checkpoint_tx,
        sweep_tx,
        drain_tx,
        sweep_trigger_gate,
    }
}

#[cfg(test)]
mod scheduling_tests;

#[cfg(test)]
fn make_worker(db: Arc<StreamsDatabase>) -> StreamWorker {
    let (_checkpoint_tx, checkpoint_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_sweep_tx, sweep_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_drain_tx, drain_rx) = tokio::sync::mpsc::unbounded_channel();
    StreamWorker::new(
        db,
        DaemonTelemetryWorkerHandle::new_noop(),
        Arc::new(Notify::new()),
        Arc::new(AtomicBool::new(false)),
        checkpoint_rx,
        sweep_rx,
        drain_rx,
        SweepTriggerGate::new(),
    )
}

#[cfg(test)]
mod subagent_sweep_tests;
