use super::*;
use crate::model::authorship_log_serialization::generate_session_id;
use std::io::Write;
use std::time::Instant;
use tempfile::TempDir;
use tokio::sync::mpsc::error::TryRecvError;

#[test]
fn no_op_worker_fixture_starts_idle() {
    let temp = TempDir::new().unwrap();
    let db = Arc::new(StreamsDatabase::open(temp.path().join("streams.db")).unwrap());
    let worker = super::make_worker(db);

    assert!(worker.checkpoint_rx.is_empty() && worker.checkpoint_rx.is_closed());
    assert!(worker.sweep_rx.is_empty() && worker.sweep_rx.is_closed());
    assert!(worker.drain_rx.is_empty() && worker.drain_rx.is_closed());
    assert_eq!(worker.telemetry_handle.metrics_buffer_len(), 0);
    assert_eq!(Arc::strong_count(&worker.shutdown_notify), 1);
    assert!(!worker.shutdown_flag.load(Ordering::Relaxed));
    let gate = &worker.sweep_trigger_gate;
    assert!(gate.try_mark_sweep_at(Instant::now(), "test"));
}

#[test]
fn triggered_sweeps_share_thirty_second_cooldown() {
    let (sweep_tx, mut sweep_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = StreamWorkerHandle::for_test_sweep_triggers(sweep_tx);
    let started_at = Instant::now();

    assert!(
        handle
            .sweep_trigger_gate
            .try_mark_sweep_at(started_at, "periodic")
    );

    assert!(!handle.trigger_sweep_at(
        SweepTrigger::PostCommit,
        started_at + Duration::from_secs(29)
    ));
    assert!(matches!(sweep_rx.try_recv(), Err(TryRecvError::Empty)));

    assert!(handle.trigger_sweep_at(
        SweepTrigger::PostCommit,
        started_at + Duration::from_secs(30)
    ));
    let request = sweep_rx.try_recv().unwrap();
    assert_eq!(request.trigger, SweepTrigger::PostCommit);
    assert_eq!(request.priority, Priority::Low);
    assert!(request.completion.is_none());

    assert!(!handle.trigger_sweep_at(SweepTrigger::PostPush, started_at + Duration::from_secs(59)));
    assert!(matches!(sweep_rx.try_recv(), Err(TryRecvError::Empty)));

    assert!(handle.trigger_sweep_at(SweepTrigger::PostPush, started_at + Duration::from_secs(60)));
    let request = sweep_rx.try_recv().unwrap();
    assert_eq!(request.trigger, SweepTrigger::PostPush);
    assert_eq!(request.priority, Priority::Low);
    assert!(request.completion.is_none());
}

#[test]
fn recovery_sweep_bypasses_cooldown_and_suppresses_followup_trigger() {
    let (sweep_tx, mut sweep_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = StreamWorkerHandle::for_test_sweep_triggers(sweep_tx);
    let started_at = Instant::now();

    assert!(
        handle
            .sweep_trigger_gate
            .try_mark_sweep_at(started_at, "periodic")
    );

    let completion = handle
        .trigger_sweep_for_recovery(SweepTrigger::PostCommit)
        .expect("recovery sweep should enqueue");
    let request = sweep_rx.try_recv().unwrap();
    assert_eq!(request.trigger, SweepTrigger::PostCommit);
    assert_eq!(request.priority, Priority::Immediate);
    assert!(request.completion.is_some());
    drop(completion);

    assert!(!handle.trigger_sweep_at(
        SweepTrigger::PostCommit,
        started_at + Duration::from_secs(29)
    ));
    assert!(matches!(sweep_rx.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn test_sweep_subagents_discovers_subagent_files() {
    let tmp = TempDir::new().unwrap();

    // Create a main session transcript: <project>/sess-abc.jsonl
    let project_dir = tmp.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let main_transcript = project_dir.join("sess-abc.jsonl");
    let mut f = std::fs::File::create(&main_transcript).unwrap();
    writeln!(f, r#"{{"type":"session","id":"sess-abc"}}"#).unwrap();

    // Create subagents directory: <project>/sess-abc/subagents/
    let subagents_dir = project_dir.join("sess-abc").join("subagents");
    std::fs::create_dir_all(&subagents_dir).unwrap();

    // Create two subagent transcripts
    let sub1 = subagents_dir.join("agent-sub1.jsonl");
    let mut f = std::fs::File::create(&sub1).unwrap();
    writeln!(
        f,
        r#"{{"type":"message","message":{{"role":"user","content":"hi"}}}}"#
    )
    .unwrap();

    let sub2 = subagents_dir.join("agent-sub2.jsonl");
    let mut f = std::fs::File::create(&sub2).unwrap();
    writeln!(
        f,
        r#"{{"type":"message","message":{{"role":"assistant","content":"hello"}}}}"#
    )
    .unwrap();

    // Also create a .meta.json file that should be ignored
    let meta = subagents_dir.join("agent-sub1.meta.json");
    std::fs::File::create(&meta).unwrap();

    // Set up worker with DB
    let db_path = tmp.path().join("test.db");
    let db = Arc::new(StreamsDatabase::open(&db_path).unwrap());
    let mut worker = make_worker(db.clone());

    let notification = CheckpointNotification {
        session_id: "internal-sess-abc".to_string(),
        tool: "claude".to_string(),
        trace_id: "trace-1".to_string(),
        tool_use_id: None,
        stream_path: main_transcript.clone(),
        stream_format: CheckpointStreamFormat::ClaudeJsonl,
        repo_work_dir: Some(tmp.path().to_path_buf()),
        external_session_id: "sess-abc".to_string(),
        external_parent_session_id: None,
    };

    worker.sweep_subagents_for_session(&notification);

    // Should have enqueued 2 subagent tasks
    assert_eq!(worker.priority_queue.len(), 2);

    // Both should be in the DB (paths are canonicalized before storage)
    let sub1_sid = generate_session_id("agent-sub1", "claude");
    let sub2_sid = generate_session_id("agent-sub2", "claude");
    let sub1_canonical = std::fs::canonicalize(&sub1).unwrap();
    let sub2_canonical = std::fs::canonicalize(&sub2).unwrap();

    let rec1 = db
        .get_stream(
            &sub1_sid,
            "transcript",
            &sub1_canonical.display().to_string(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(rec1.external_session_id, "agent-sub1");
    assert_eq!(rec1.external_parent_session_id.as_deref(), Some("sess-abc"));
    assert_eq!(rec1.tool, "claude");

    let rec2 = db
        .get_stream(
            &sub2_sid,
            "transcript",
            &sub2_canonical.display().to_string(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(rec2.external_session_id, "agent-sub2");
    assert_eq!(rec2.external_parent_session_id.as_deref(), Some("sess-abc"));
}

#[test]
fn test_sweep_subagents_no_dir_is_noop() {
    let tmp = TempDir::new().unwrap();
    let main_transcript = tmp.path().join("sess-xyz.jsonl");
    let mut f = std::fs::File::create(&main_transcript).unwrap();
    writeln!(f, r#"{{"type":"session"}}"#).unwrap();

    let db_path = tmp.path().join("test.db");
    let db = Arc::new(StreamsDatabase::open(&db_path).unwrap());
    let mut worker = make_worker(db.clone());

    let notification = CheckpointNotification {
        session_id: "internal-sess-xyz".to_string(),
        tool: "claude".to_string(),
        trace_id: "trace-2".to_string(),
        tool_use_id: None,
        stream_path: main_transcript.clone(),
        stream_format: CheckpointStreamFormat::ClaudeJsonl,
        repo_work_dir: None,
        external_session_id: "sess-xyz".to_string(),
        external_parent_session_id: None,
    };

    worker.sweep_subagents_for_session(&notification);
    assert_eq!(worker.priority_queue.len(), 0);
}

#[test]
fn test_sweep_subagents_deduplicates_in_flight() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();

    let main_transcript = project_dir.join("sess-dup.jsonl");
    std::fs::File::create(&main_transcript).unwrap();

    let subagents_dir = project_dir.join("sess-dup").join("subagents");
    std::fs::create_dir_all(&subagents_dir).unwrap();
    let sub = subagents_dir.join("agent-inflight.jsonl");
    let mut f = std::fs::File::create(&sub).unwrap();
    writeln!(f, r#"{{"type":"message"}}"#).unwrap();

    let db_path = tmp.path().join("test.db");
    let db = Arc::new(StreamsDatabase::open(&db_path).unwrap());
    let mut worker = make_worker(db.clone());

    // Mark the subagent's canonical path as in-flight
    let canonical = std::fs::canonicalize(&sub).unwrap();
    worker
        .in_flight
        .insert((canonical, "transcript".to_string()));

    let notification = CheckpointNotification {
        session_id: "internal-sess-dup".to_string(),
        tool: "claude".to_string(),
        trace_id: "trace-4".to_string(),
        tool_use_id: None,
        stream_path: main_transcript,
        stream_format: CheckpointStreamFormat::ClaudeJsonl,
        repo_work_dir: None,
        external_session_id: "sess-dup".to_string(),
        external_parent_session_id: None,
    };

    worker.sweep_subagents_for_session(&notification);

    // Should not enqueue the in-flight subagent
    assert_eq!(worker.priority_queue.len(), 0);
}
