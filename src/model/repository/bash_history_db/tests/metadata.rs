use super::*;

fn start(trace: &str, marker: &str) -> BashCallStart {
    BashCallStart {
        original_cwd: "/repo".into(),
        repo_work_dir: Some("/repo".into()),
        repo_discovery_error: None,
        session_id: "session".into(),
        tool_use_id: "tool".into(),
        agent_id: test_agent(),
        start_trace_id: trace.into(),
        started_at_ns: unix_time_ns(),
        command: Some("true".into()),
        metadata: HashMap::from([
            ("start_only".into(), marker.into()),
            ("shared".into(), "old".into()),
        ]),
    }
}

fn end(trace: Option<&str>) -> BashCallEnd {
    BashCallEnd {
        original_cwd: "/repo".into(),
        repo_work_dir: Some("/repo".into()),
        repo_discovery_error: None,
        session_id: "session".into(),
        tool_use_id: "tool".into(),
        agent_id: test_agent(),
        start_trace_id: trace.map(str::to_owned),
        end_trace_id: "end".into(),
        started_at_ns: None,
        ended_at_ns: unix_time_ns(),
        command: None,
        metadata: HashMap::from([
            ("shared".into(), "new".into()),
            ("end_only".into(), "added".into()),
        ]),
    }
}

#[test]
fn explicit_trace_merges_only_the_matching_invocation() {
    let (mut db, _dir) = test_db();
    db.record_start(&start("first", "first marker")).unwrap();
    db.record_start(&start("second", "second marker")).unwrap();
    db.record_end(&end(Some("first"))).unwrap();
    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].metadata["start_only"], "first marker");
    assert_eq!(calls[0].metadata["shared"], "new");
    assert_eq!(calls[0].metadata["end_only"], "added");
    assert_eq!(calls[1].metadata, start("second", "second marker").metadata);
    assert!(calls[1].end_time_ns.is_none());
}

#[test]
fn implicit_completion_merges_only_the_latest_open_invocation() {
    let (mut db, _dir) = test_db();
    db.record_start(&start("first", "first marker")).unwrap();
    db.record_start(&start("second", "second marker")).unwrap();
    db.record_end(&end(None)).unwrap();
    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls[0].metadata, start("first", "first marker").metadata);
    assert!(calls[0].end_time_ns.is_none());
    assert_eq!(calls[1].metadata["start_only"], "second marker");
    assert_eq!(calls[1].metadata["shared"], "new");
    assert!(calls[1].end_time_ns.is_some());
}

#[test]
fn duplicate_completion_preserves_prior_metadata_and_updates_present_fields() {
    let (mut db, _dir) = test_db();
    db.record_start(&start("first", "first marker")).unwrap();
    db.record_end(&end(Some("first"))).unwrap();
    let mut repeated = end(Some("first"));
    repeated.metadata = HashMap::from([("shared".into(), "last".into())]);
    db.record_end(&repeated).unwrap();
    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].metadata["start_only"], "first marker");
    assert_eq!(calls[0].metadata["end_only"], "added");
    assert_eq!(calls[0].metadata["shared"], "last");
}

#[test]
fn unmatched_end_cannot_borrow_another_invocations_metadata() {
    let (mut db, _dir) = test_db();
    db.record_start(&start("first", "first marker")).unwrap();
    db.record_end(&end(Some("missing"))).unwrap();
    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].metadata, start("first", "first marker").metadata);
    assert_eq!(calls[1].metadata, end(Some("missing")).metadata);
}

#[test]
fn malformed_legacy_metadata_does_not_drop_completion_fields() {
    let (mut db, _dir) = test_db();
    db.record_start(&start("first", "first marker")).unwrap();
    db.conn
        .execute(
            "UPDATE bash_checkpoint_calls SET metadata_json = 'invalid'",
            [],
        )
        .unwrap();
    db.record_end(&end(Some("first"))).unwrap();
    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls[0].metadata, end(Some("first")).metadata);
}

#[test]
fn timestamp_overflow_retains_the_persisted_error_text_and_type() {
    let error = ns_to_i64(u128::MAX).unwrap_err();
    assert!(matches!(error, GitAiError::Persistence(_)));
    assert_eq!(
        error.to_string(),
        format!("Generic error: timestamp too large: {}", u128::MAX)
    );
}
