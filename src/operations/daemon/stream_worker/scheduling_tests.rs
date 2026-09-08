use super::*;
use crate::model::stream_types::StreamError;
use tempfile::TempDir;

fn make_worker() -> (
    TempDir,
    StreamWorker,
    Arc<Notify>,
    tokio::sync::mpsc::UnboundedSender<CheckpointNotification>,
) {
    let temp = TempDir::new().unwrap();
    let db = Arc::new(StreamsDatabase::open(temp.path().join("streams.db")).unwrap());
    let (checkpoint_tx, checkpoint_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_sweep_tx, sweep_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_drain_tx, drain_rx) = tokio::sync::mpsc::unbounded_channel();
    let shutdown = Arc::new(Notify::new());
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let telemetry = DaemonTelemetryWorkerHandle::new_noop();
    let sweep_trigger_gate = SweepTriggerGate::new();
    assert!(sweep_trigger_gate.try_mark_sweep_at(Instant::now(), "test"));
    let worker = StreamWorker::new(
        db,
        telemetry,
        shutdown.clone(),
        shutdown_flag,
        checkpoint_rx,
        sweep_rx,
        drain_rx,
        sweep_trigger_gate,
    );
    (temp, worker, shutdown, checkpoint_tx)
}

fn task(session_id: &str, next_retry_at: Option<std::time::Instant>) -> ProcessingTask {
    ProcessingTask {
        priority: Priority::Immediate,
        session_id: session_id.to_string(),
        stream_kind: "transcript".to_string(),
        tool: "test".to_string(),
        trace_id: None,
        tool_use_id: None,
        canonical_path: PathBuf::from(format!("/test/{session_id}")),
        repo_work_dir: None,
        retry_count: 0,
        next_retry_at,
    }
}

#[test]
fn promotes_only_ready_delayed_tasks() {
    let (_temp, mut worker, _shutdown, _checkpoint_tx) = make_worker();
    let now = std::time::Instant::now();
    worker.delayed_tasks.push(task("ready", Some(now)));
    worker
        .delayed_tasks
        .push(task("future", Some(now + Duration::from_secs(5))));

    worker.promote_ready_delayed_tasks(now);

    assert_eq!(worker.priority_queue.len(), 1);
    assert_eq!(worker.priority_queue.peek().unwrap().session_id, "ready");
    assert_eq!(worker.delayed_tasks.len(), 1);
    assert_eq!(worker.delayed_tasks[0].session_id, "future");
}

#[test]
fn next_delayed_task_at_returns_earliest_retry_deadline() {
    let (_temp, mut worker, _shutdown, _checkpoint_tx) = make_worker();
    let now = std::time::Instant::now();
    let later = now + Duration::from_secs(30);
    let earlier = now + Duration::from_secs(5);
    worker.delayed_tasks.push(task("later", Some(later)));
    worker.delayed_tasks.push(task("earlier", Some(earlier)));

    assert_eq!(worker.next_delayed_task_at(), Some(earlier));
}

#[test]
fn take_immediate_tasks_preserves_low_priority_tasks() {
    let (_temp, mut worker, _shutdown, _checkpoint_tx) = make_worker();
    let immediate = task("immediate", None);
    let mut queued_low = task("queued-low", None);
    queued_low.priority = Priority::Low;
    let mut delayed_low = task("delayed-low", None);
    delayed_low.priority = Priority::Low;

    worker.priority_queue.push(immediate.clone());
    worker.priority_queue.push(queued_low.clone());
    worker.delayed_tasks.push(delayed_low.clone());

    assert_eq!(worker.take_immediate_tasks(), vec![immediate]);
    assert_eq!(worker.priority_queue.into_vec(), vec![queued_low]);
    assert_eq!(worker.delayed_tasks, vec![delayed_low]);
}

#[tokio::test]
async fn transient_errors_are_stored_as_delayed_retries_not_ready_work() {
    let (_temp, mut worker, _shutdown, _checkpoint_tx) = make_worker();

    worker
        .handle_processing_error(
            task("retry", None),
            StreamError::Transient {
                message: "temporary".to_string(),
                retry_after: Duration::from_secs(1),
            },
        )
        .await;

    assert!(worker.priority_queue.is_empty());
    assert_eq!(worker.delayed_tasks.len(), 1);
    assert_eq!(worker.delayed_tasks[0].session_id, "retry");
    assert_eq!(worker.delayed_tasks[0].retry_count, 1);
    assert!(worker.delayed_tasks[0].next_retry_at.is_some());
}

#[tokio::test]
async fn shutdown_wakes_idle_worker() {
    let (_temp, worker, shutdown, _checkpoint_tx) = make_worker();
    let handle = tokio::spawn(worker.run());
    tokio::task::yield_now().await;

    shutdown.notify_one();

    tokio::time::timeout(Duration::from_secs(1), handle)
        .await
        .expect("idle worker should exit promptly after shutdown notification")
        .expect("worker task should not panic");
}

#[tokio::test]
async fn shutdown_wakes_worker_sleeping_until_future_retry() {
    let (_temp, mut worker, shutdown, _checkpoint_tx) = make_worker();
    worker.delayed_tasks.push(task(
        "future-retry",
        Some(std::time::Instant::now() + Duration::from_secs(60 * 60)),
    ));
    let handle = tokio::spawn(worker.run());
    tokio::task::yield_now().await;

    shutdown.notify_one();

    tokio::time::timeout(Duration::from_secs(1), handle)
        .await
        .expect("retry-sleeping worker should exit promptly after shutdown notification")
        .expect("worker task should not panic");
}

#[tokio::test]
async fn drain_processes_queued_checkpoint_notifications_before_completing() {
    let (temp, mut worker, _shutdown, checkpoint_tx) = make_worker();
    checkpoint_tx
        .send(CheckpointNotification {
            session_id: "session".to_string(),
            tool: "unknown".to_string(),
            trace_id: "trace".to_string(),
            tool_use_id: None,
            stream_path: temp.path().join("transcript.jsonl"),
            stream_format: CheckpointStreamFormat::ClaudeJsonl,
            repo_work_dir: Some(temp.path().to_path_buf()),
            external_session_id: "external".to_string(),
            external_parent_session_id: None,
        })
        .unwrap();
    let (completion_tx, completion_rx) = tokio::sync::oneshot::channel();

    worker
        .handle_drain_request(
            DrainRequest {
                completion: completion_tx,
            },
            false,
        )
        .await;

    completion_rx.await.unwrap();
    assert!(
        worker.checkpoint_rx.is_empty(),
        "drain must process checkpoint notifications queued before the barrier"
    );
}
