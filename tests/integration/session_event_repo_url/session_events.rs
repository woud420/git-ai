use super::{
    ByteOffsetWatermark, ClaudeAgent, EventAttributes, MetricEvent, PathBuf, SessionEventValues,
    TempDir, TestRepo, make_stream_record, resolve_repo_url_from_path, setup_test_db,
    write_claude_transcript_with_cwd, write_claude_transcript_without_cwd,
};
use git_ai::metrics::PosEncoded;
use git_ai::operations::streams::agent::Agent;

// === Test Group 4: Session Event repo_url from Hook Path ===

#[test]
fn test_session_events_include_repo_url_from_hook_triggered_checkpoint() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "git@github.com:acme/app.git"])
        .unwrap();

    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.jsonl");
    write_claude_transcript_with_cwd(&transcript, repo.path().to_str().unwrap());

    let db = setup_test_db(temp_dir.path());
    let record = make_stream_record(
        "test-hook-sess",
        "claude",
        &transcript,
        Some(repo.path().to_str().unwrap()),
    );
    db.insert_stream(&record).unwrap();

    let session = db
        .get_stream(
            "test-hook-sess",
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    let repo_work_dir = session.repo_work_dir.as_ref().map(PathBuf::from);
    let resolved_repo_url = repo_work_dir
        .as_ref()
        .and_then(|p| resolve_repo_url_from_path(p));

    assert_eq!(
        resolved_repo_url,
        Some("https://github.com/acme/app".to_string()),
        "Session events from hook-triggered checkpoints MUST include repo_url"
    );

    // Build attrs and verify
    let mut base_attrs = EventAttributes::with_version("test")
        .session_id(session.session_id.clone())
        .tool(&session.tool)
        .external_session_id(session.external_session_id.clone());
    if let Some(url) = &resolved_repo_url {
        base_attrs = base_attrs.repo_url(url.clone());
    }
    let sparse = base_attrs.to_sparse();
    let attrs = EventAttributes::from_sparse(&sparse);

    assert_eq!(
        attrs.repo_url,
        Some(Some("https://github.com/acme/app".to_string())),
        "EventAttributes must carry repo_url through sparse encoding"
    );
    assert_eq!(attrs.tool, Some(Some("claude".to_string())));
    assert_eq!(attrs.session_id, Some(Some("test-hook-sess".to_string())));
}

#[test]
fn test_session_events_no_repo_url_when_no_remote() {
    let repo = TestRepo::new();

    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.jsonl");
    write_claude_transcript_with_cwd(&transcript, repo.path().to_str().unwrap());

    let db = setup_test_db(temp_dir.path());
    let record = make_stream_record(
        "test-no-remote",
        "claude",
        &transcript,
        Some(repo.path().to_str().unwrap()),
    );
    db.insert_stream(&record).unwrap();

    let session = db
        .get_stream(
            "test-no-remote",
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    let repo_work_dir = session.repo_work_dir.as_ref().map(PathBuf::from);
    let resolved = repo_work_dir
        .as_ref()
        .and_then(|p| resolve_repo_url_from_path(p));

    assert_eq!(
        resolved, None,
        "repo_url must be None when repo has no remote"
    );
}

#[test]
fn test_session_events_no_repo_url_when_no_work_dir() {
    let temp_dir = TempDir::new().unwrap();
    let transcript = temp_dir.path().join("session.jsonl");
    write_claude_transcript_without_cwd(&transcript);

    let db = setup_test_db(temp_dir.path());
    let record = make_stream_record("test-no-workdir", "claude", &transcript, None);
    db.insert_stream(&record).unwrap();

    let session = db
        .get_stream(
            "test-no-workdir",
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(session.repo_work_dir, None);

    let resolved = None::<PathBuf>
        .as_ref()
        .and_then(|p: &PathBuf| resolve_repo_url_from_path(p));
    assert_eq!(
        resolved, None,
        "repo_url must be None when no work_dir and no cwd inference"
    );
}

// === Test Group 5: Session Event repo_url from Sweep Path (cwd inference) ===

#[test]
fn test_session_events_repo_url_from_sweep_inferred_cwd() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "git@github.com:team/service.git"])
        .unwrap();

    let transcript = repo.path().join("transcript.jsonl");
    write_claude_transcript_with_cwd(&transcript, repo.path().to_str().unwrap());

    let temp_dir = TempDir::new().unwrap();
    let db = setup_test_db(temp_dir.path());
    let record = make_stream_record("test-sweep-sess", "claude", &transcript, None);
    db.insert_stream(&record).unwrap();

    let session = db
        .get_stream(
            "test-sweep-sess",
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        session.repo_work_dir, None,
        "Sweep should not have repo_work_dir initially"
    );

    let agent = ClaudeAgent::new();
    let inferred_cwd = agent.infer_cwd(&PathBuf::from(&session.stream_path));
    assert_eq!(
        inferred_cwd,
        Some(repo.path().to_path_buf()),
        "Claude agent must infer cwd from transcript"
    );

    let resolved = inferred_cwd
        .as_ref()
        .and_then(|p| resolve_repo_url_from_path(p));
    assert_eq!(
        resolved,
        Some("https://github.com/team/service".to_string()),
        "Sweep-discovered sessions MUST resolve repo_url when cwd is inferable"
    );
}

// === Test Group 8: Full E2E pipeline with repo_url ===

#[test]
fn test_full_pipeline_session_events_carry_repo_url() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "git@github.com:myorg/myrepo.git"])
        .unwrap();

    let temp_dir = TempDir::new().unwrap();
    let db = setup_test_db(temp_dir.path());

    let transcript = temp_dir.path().join("session.jsonl");
    write_claude_transcript_with_cwd(&transcript, repo.path().to_str().unwrap());

    let record = make_stream_record(
        "pipeline-test",
        "claude",
        &transcript,
        Some(repo.path().to_str().unwrap()),
    );
    db.insert_stream(&record).unwrap();

    // Replicate process_session_blocking logic
    let retrieved = db
        .get_stream(
            "pipeline-test",
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    let agent = ClaudeAgent::new();
    let watermark = Box::new(ByteOffsetWatermark::new(0));
    let batch = agent
        .read_incremental(
            &PathBuf::from(&retrieved.stream_path),
            watermark,
            &retrieved.session_id,
        )
        .unwrap();

    // Resolve repo_url
    let repo_work_dir = retrieved.repo_work_dir.as_ref().map(PathBuf::from);
    let resolved_url = repo_work_dir
        .as_ref()
        .and_then(|p| resolve_repo_url_from_path(p));

    // Build attrs
    let mut base_attrs = EventAttributes::with_version(env!("CARGO_PKG_VERSION"))
        .session_id(retrieved.session_id.clone())
        .tool(&retrieved.tool)
        .external_session_id(retrieved.external_session_id.clone())
        .external_parent_session_id_opt(retrieved.external_parent_session_id.clone());
    if let Some(ref url) = resolved_url {
        base_attrs = base_attrs.repo_url(url.clone());
    }

    // Build events
    let metric_events: Vec<MetricEvent> = batch
        .events
        .into_iter()
        .map(|raw_event| {
            let (eid, pid, tid) = agent.extract_event_ids(&raw_event);
            let sparse = base_attrs
                .clone()
                .trace_id("test-trace".to_string())
                .to_sparse();
            MetricEvent::from_values(
                SessionEventValues::with_ids(raw_event, eid, pid, tid),
                sparse,
            )
        })
        .collect();

    assert!(
        !metric_events.is_empty(),
        "Should have produced session events"
    );

    // STRICT: Every single event must have repo_url
    for (i, event) in metric_events.iter().enumerate() {
        let attrs = EventAttributes::from_sparse(&event.attrs);
        assert_eq!(
            attrs.repo_url,
            Some(Some("https://github.com/myorg/myrepo".to_string())),
            "Event {} must have repo_url set",
            i
        );
        assert_eq!(
            attrs.tool,
            Some(Some("claude".to_string())),
            "Event {} must have tool set",
            i
        );
        assert_eq!(
            attrs.session_id,
            Some(Some("pipeline-test".to_string())),
            "Event {} must have session_id set",
            i
        );
    }
}

#[test]
fn test_full_pipeline_session_events_no_repo_url_when_unavailable() {
    let temp_dir = TempDir::new().unwrap();
    let db = setup_test_db(temp_dir.path());

    let transcript = temp_dir.path().join("session.jsonl");
    write_claude_transcript_without_cwd(&transcript);

    let record = make_stream_record("no-repo-test", "claude", &transcript, None);
    db.insert_stream(&record).unwrap();

    let retrieved = db
        .get_stream(
            "no-repo-test",
            "transcript",
            &transcript.display().to_string(),
        )
        .unwrap()
        .unwrap();
    let agent = ClaudeAgent::new();
    let watermark = Box::new(ByteOffsetWatermark::new(0));
    let batch = agent
        .read_incremental(
            &PathBuf::from(&retrieved.stream_path),
            watermark,
            &retrieved.session_id,
        )
        .unwrap();

    let repo_work_dir = retrieved.repo_work_dir.as_ref().map(PathBuf::from);
    let inferred = agent.infer_cwd(&PathBuf::from(&retrieved.stream_path));
    let resolved_work_dir = repo_work_dir.or(inferred);
    let resolved_url = resolved_work_dir
        .as_ref()
        .and_then(|p| resolve_repo_url_from_path(p));

    assert_eq!(
        resolved_url, None,
        "No repo_url should be resolved when unavailable"
    );

    // Build attrs without repo_url
    let base_attrs = EventAttributes::with_version("test")
        .session_id(retrieved.session_id.clone())
        .tool(&retrieved.tool);

    let metric_events: Vec<MetricEvent> = batch
        .events
        .into_iter()
        .map(|raw_event| {
            let (eid, pid, tid) = agent.extract_event_ids(&raw_event);
            let sparse = base_attrs.clone().to_sparse();
            MetricEvent::from_values(
                SessionEventValues::with_ids(raw_event, eid, pid, tid),
                sparse,
            )
        })
        .collect();

    assert!(!metric_events.is_empty(), "Should have produced events");

    for (i, event) in metric_events.iter().enumerate() {
        let attrs = EventAttributes::from_sparse(&event.attrs);
        assert!(
            attrs.repo_url.is_none() || matches!(&attrs.repo_url, Some(None)),
            "Event {} must NOT have repo_url when unavailable, got: {:?}",
            i,
            attrs.repo_url
        );
    }
}
