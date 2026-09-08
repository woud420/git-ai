use super::{
    Agent, Arc, CopilotAgent, EventAttributes, MetricEvent, OtelTraceValues, PathBuf, PosEncoded,
    StreamFormat, StreamRecord, StreamsDatabase, TempDir, TimestampCursorWatermark,
    WatermarkStrategy, WatermarkType, fixture_path,
};

#[test]
fn test_copilot_otel_stream_reads_spans_with_event_ids() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("transcripts.db");
    let db = Arc::new(StreamsDatabase::open(&db_path).unwrap());

    let fixture = fixture_path("copilot-otel/traces.db");
    let now = chrono::Utc::now().timestamp();

    // Create session record for the OTEL stream
    let session = StreamRecord {
        session_id: "copilot-otel-test-session".to_string(),
        stream_kind: "otel_traces".to_string(),
        tool: "github-copilot".to_string(),
        stream_path: fixture.display().to_string(),
        stream_format: StreamFormat::CopilotOtelSqlite,
        watermark_type: WatermarkType::TimestampCursor,
        watermark_value: TimestampCursorWatermark::initial().serialize(),
        external_session_id: "copilot-ext-session-1".to_string(),
        external_parent_session_id: None,
        first_seen_at: now,
        last_processed_at: 0,
        last_known_size: 0,
        last_modified: None,
        processing_errors: 0,
        last_error: None,
        repo_work_dir: None,
    };
    db.insert_stream(&session).unwrap();

    // Read spans using CopilotAgent (dispatches to copilot_otel reader for .db files)
    let agent = CopilotAgent::new();
    let watermark = Box::new(TimestampCursorWatermark::initial());
    let batch = agent
        .read_incremental(
            &PathBuf::from(&session.stream_path),
            watermark,
            &session.session_id,
        )
        .unwrap();

    // The fixture has 24 spans
    assert!(!batch.events.is_empty(), "expected spans from fixture DB");
    assert!(
        batch.events.len() >= 20,
        "fixture has ~24 spans, got {}",
        batch.events.len()
    );

    // Verify event structure
    let first = &batch.events[0];
    assert!(first.get("span").is_some(), "event should have 'span' key");
    assert!(
        first.get("attributes").is_some(),
        "event should have 'attributes' key"
    );
    assert!(
        first.get("events").is_some(),
        "event should have 'events' key"
    );

    // Verify event ID extraction works for OTEL events
    let (event_id, _parent_id, _tool_use_id) = agent.extract_event_ids(first);
    assert!(
        event_id.is_some(),
        "span_id should be extracted as event_id"
    );

    // Verify we can construct MetricEvents from these
    let attrs_sparse = EventAttributes::with_version("test")
        .session_id(session.session_id.clone())
        .external_session_id(session.external_session_id.clone())
        .to_sparse();

    let metric_events: Vec<MetricEvent> = batch
        .events
        .into_iter()
        .map(|raw_event| {
            let (eid, pid, tid) = agent.extract_event_ids(&raw_event);
            MetricEvent::from_values(
                OtelTraceValues::with_ids(raw_event, eid, pid, tid),
                attrs_sparse.clone(),
            )
        })
        .collect();

    assert!(!metric_events.is_empty());

    // Verify watermark advanced
    let new_wm_serialized = batch.new_watermark.serialize();
    assert_ne!(
        new_wm_serialized, "0|",
        "watermark should have advanced from initial"
    );
}

#[test]
fn test_copilot_otel_stream_watermark_resumes_correctly() {
    let fixture = fixture_path("copilot-otel/traces.db");
    let agent = CopilotAgent::new();

    // First read: get all spans from initial cursor
    let watermark1 = Box::new(TimestampCursorWatermark::initial());
    let batch1 = agent
        .read_incremental(&fixture, watermark1, "test-session")
        .unwrap();
    let count1 = batch1.events.len();
    assert!(
        count1 >= 20,
        "expected bulk of spans in first read, got {}",
        count1
    );

    // Watermark should have advanced from initial
    let wm1_str = batch1.new_watermark.serialize();
    assert_ne!(
        wm1_str, "0|",
        "watermark should advance from initial after first read"
    );

    // Second read: keyset pagination guarantees no duplicates
    let batch2 = agent
        .read_incremental(&fixture, batch1.new_watermark, "test-session")
        .unwrap();

    // With keyset pagination, second read should return remaining spans (if any)
    // or be empty if all spans were consumed in the first batch
    assert!(
        batch2.events.len() < count1,
        "second read ({}) should be smaller than first ({})",
        batch2.events.len(),
        count1
    );
}

#[test]
fn test_copilot_agent_streams_declares_otel_stream() {
    let agent = CopilotAgent::new();
    let streams = agent.streams();

    assert_eq!(streams.len(), 2, "CopilotAgent should declare 2 streams");
    assert_eq!(streams[0].stream_kind, "transcript");
    assert_eq!(streams[1].stream_kind, "otel_traces");
}

#[test]
fn test_copilot_otel_events_use_otel_trace_event_type() {
    use git_ai::metrics::OtelTraceValues;
    use git_ai::metrics::events::otel_trace_pos;

    let fixture = fixture_path("copilot-otel/traces.db");
    let agent = CopilotAgent::new();

    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = agent
        .read_incremental(&fixture, watermark, "test-session")
        .unwrap();

    assert!(!batch.events.is_empty());

    // Verify OtelTraceValues roundtrip for actual fixture data
    for raw_event in batch.events.iter().take(5) {
        let (eid, pid, tid) = agent.extract_event_ids(raw_event);
        let values =
            OtelTraceValues::with_ids(raw_event.clone(), eid.clone(), pid.clone(), tid.clone());

        // Verify sparse encoding preserves the full nested OTEL structure
        let sparse = git_ai::metrics::PosEncoded::to_sparse(&values);
        let raw_json = sparse.get(&otel_trace_pos::RAW_JSON.to_string()).unwrap();
        assert!(
            raw_json.get("span").is_some(),
            "raw_json must contain 'span' key"
        );
        assert!(
            raw_json.get("attributes").is_some(),
            "raw_json must contain 'attributes' key"
        );
        assert!(
            raw_json.get("events").is_some(),
            "raw_json must contain 'events' key"
        );

        // Verify IDs are preserved
        if let Some(ref id) = eid {
            assert_eq!(
                sparse.get(&otel_trace_pos::EXTERNAL_EVENT_ID.to_string()),
                Some(&serde_json::json!(id))
            );
        }
    }
}

#[test]
fn test_copilot_otel_per_event_session_id_derivation() {
    use git_ai::model::authorship_log_serialization::generate_session_id;

    let fixture = fixture_path("copilot-otel/traces.db");
    let agent = CopilotAgent::new();

    let watermark: Box<dyn WatermarkStrategy> = Box::new(TimestampCursorWatermark::initial());
    let batch = agent
        .read_incremental(&fixture, watermark, "test-session")
        .unwrap();

    // Every event from the fixture should have an extractable session_id
    // (the SQL filter already excludes spans without session IDs)
    for event in &batch.events {
        let session_id = agent.extract_event_session_id(event);
        assert!(
            session_id.is_some(),
            "fixture spans should all have extractable session_id, span: {}",
            event["span"]["span_id"]
        );

        // Verify the derived session_id is deterministic
        let sid = session_id.unwrap();
        let derived1 = generate_session_id(&sid, "github-copilot");
        let derived2 = generate_session_id(&sid, "github-copilot");
        assert_eq!(
            derived1, derived2,
            "session_id derivation must be deterministic"
        );
    }
}

#[test]
fn test_copilot_agent_streams_otel_path_resolution() {
    use git_ai::operations::streams::agent::Agent;

    let agent = CopilotAgent::new();
    let streams = agent.streams();

    // First stream is transcript (identity path)
    let transcript_stream = &streams[0];
    assert_eq!(transcript_stream.stream_kind, "transcript");
    assert!(!transcript_stream.shared);

    let test_path = std::path::PathBuf::from("/fake/path/transcripts/session.jsonl");
    let resolved = transcript_stream.resolve_path(&test_path);
    assert_eq!(resolved, Some(test_path.clone()));

    // Second stream is otel_traces (shared, custom resolver)
    let otel_stream = &streams[1];
    assert_eq!(otel_stream.stream_kind, "otel_traces");
    assert!(otel_stream.shared);
}
