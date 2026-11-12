use super::*;
use tempfile::TempDir;

fn drain_request() -> (DrainRequest, tokio::sync::oneshot::Receiver<()>) {
    let (completion, completed) = tokio::sync::oneshot::channel();
    (DrainRequest { completion }, completed)
}

fn immediate_task(path: PathBuf) -> ProcessingTask {
    ProcessingTask {
        priority: Priority::Immediate,
        session_id: SHARED_STREAM_SESSION_ID.to_string(),
        stream_kind: "transcript".to_string(),
        tool: "claude".to_string(),
        trace_id: None,
        tool_use_id: None,
        canonical_path: path,
        repo_work_dir: None,
        retry_count: 0,
        next_retry_at: None,
    }
}

#[tokio::test]
async fn drain_keeps_a_transiently_failed_immediate_task_queued_for_retry() {
    let temp = TempDir::new().unwrap();
    let db = Arc::new(StreamsDatabase::open(temp.path().join("streams.db")).unwrap());
    let mut worker = make_worker(db);

    // A record over the line budget is a transient read failure on every platform.
    let transcript = temp.path().join("oversized.jsonl");
    let oversized = config::Config::get().max_transcript_line_bytes() + 1;
    std::fs::write(&transcript, "x".repeat(oversized)).unwrap();
    worker
        .ensure_stream_session(
            SHARED_STREAM_SESSION_ID,
            "claude",
            &StreamDescriptor::identity_transcript(StreamFormat::ClaudeJsonl),
            &transcript,
            None,
            None,
            None,
        )
        .unwrap();
    worker.priority_queue.push(immediate_task(transcript));

    let (request, completed) = drain_request();
    worker.handle_drain_request(request, false).await;
    completed.await.unwrap();

    assert!(worker.priority_queue.is_empty());
    assert_eq!(
        worker.delayed_tasks.len(),
        1,
        "a transient failure during an await drain must be retried, not dropped"
    );
    assert_eq!(worker.delayed_tasks[0].retry_count, 1);
    assert!(worker.delayed_tasks[0].next_retry_at.is_some());
}

#[tokio::test]
async fn shutdown_drain_finishes_immediate_tasks_after_shutdown_is_requested() {
    let temp = TempDir::new().unwrap();
    let db = Arc::new(StreamsDatabase::open(temp.path().join("streams.db")).unwrap());
    let mut worker = make_worker(db.clone());
    let transcript = temp.path().join("session.jsonl");
    std::fs::write(
        &transcript,
        "{\"type\":\"user\",\"sessionId\":\"s\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    )
    .unwrap();
    worker
        .ensure_stream_session(
            SHARED_STREAM_SESSION_ID,
            "claude",
            &StreamDescriptor::identity_transcript(StreamFormat::ClaudeJsonl),
            &transcript,
            None,
            None,
            None,
        )
        .unwrap();
    worker
        .priority_queue
        .push(immediate_task(transcript.clone()));

    // Shutdown stops sessions already in progress, but not the final drain.
    worker.shutdown_flag.store(true, Ordering::Relaxed);
    worker.drain_immediate_tasks().await;

    let stream = db
        .get_stream(
            SHARED_STREAM_SESSION_ID,
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    assert_ne!(
        stream.watermark_value, "0",
        "drain must read the transcript"
    );
}

#[tokio::test]
async fn drain_leaves_immediate_tasks_that_are_backing_off_for_their_retry() {
    let temp = TempDir::new().unwrap();
    let db = Arc::new(StreamsDatabase::open(temp.path().join("streams.db")).unwrap());
    let mut worker = make_worker(db);
    let mut backing_off = immediate_task(temp.path().join("retrying.jsonl"));
    backing_off.retry_count = 1;
    backing_off.next_retry_at = Some(Instant::now() + Duration::from_secs(60 * 60));
    worker.delayed_tasks.push(backing_off.clone());

    let (request, completed) = drain_request();
    worker.handle_drain_request(request, false).await;
    completed.await.unwrap();

    assert_eq!(worker.delayed_tasks, vec![backing_off]);
}
