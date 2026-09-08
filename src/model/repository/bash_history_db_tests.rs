use super::*;

fn test_agent() -> AgentId {
    AgentId {
        tool: "codex".to_string(),
        id: "session-1".to_string(),
        model: "gpt-5".to_string(),
    }
}

fn test_db() -> (BashHistoryDatabase, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = BashHistoryDatabase::open_at_path(&dir.path().join("bash.db")).unwrap();
    (db, dir)
}

#[test]
fn start_and_end_lifecycle_persists_fields() {
    let (mut db, _dir) = test_db();
    let mut metadata = HashMap::new();
    metadata.insert("transcript_path".to_string(), "/tmp/t.jsonl".to_string());

    db.record_start(&BashCallStart {
        original_cwd: "/repo/subdir".to_string(),
        repo_work_dir: Some("/repo".to_string()),
        repo_discovery_error: None,
        session_id: "session-1".to_string(),
        tool_use_id: "tool-1".to_string(),
        agent_id: test_agent(),
        start_trace_id: "t_start".to_string(),
        started_at_ns: 1_000,
        command: Some("echo hi".to_string()),
        metadata: metadata.clone(),
    })
    .unwrap();
    db.record_end(&BashCallEnd {
        original_cwd: "/repo/subdir".to_string(),
        repo_work_dir: Some("/repo".to_string()),
        repo_discovery_error: None,
        session_id: "session-1".to_string(),
        tool_use_id: "tool-1".to_string(),
        agent_id: test_agent(),
        start_trace_id: Some("t_start".to_string()),
        end_trace_id: "t_end".to_string(),
        started_at_ns: Some(1_000),
        ended_at_ns: 2_000,
        command: Some("echo hi".to_string()),
        metadata,
    })
    .unwrap();

    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.original_cwd, "/repo/subdir");
    assert_eq!(call.repo_work_dir.as_deref(), Some("/repo"));
    assert_eq!(call.repo_discovery_error, None);
    assert_eq!(call.session_id, "session-1");
    assert_eq!(call.tool_use_id, "tool-1");
    assert_eq!(call.agent_id, test_agent());
    assert_eq!(call.start_trace_id.as_deref(), Some("t_start"));
    assert_eq!(call.end_trace_id.as_deref(), Some("t_end"));
    assert_eq!(call.start_time_ns, 1_000);
    assert_eq!(call.end_time_ns, Some(2_000));
    assert_eq!(call.command.as_deref(), Some("echo hi"));
    assert_eq!(
        call.metadata.get("transcript_path").map(String::as_str),
        Some("/tmp/t.jsonl")
    );
}

#[test]
fn end_without_start_upserts_best_effort_row() {
    let (mut db, _dir) = test_db();

    db.record_end(&BashCallEnd {
        original_cwd: "/repo".to_string(),
        repo_work_dir: Some("/repo".to_string()),
        repo_discovery_error: None,
        session_id: "session-2".to_string(),
        tool_use_id: "tool-2".to_string(),
        agent_id: test_agent(),
        start_trace_id: None,
        end_trace_id: "t_end".to_string(),
        started_at_ns: None,
        ended_at_ns: 5_000,
        command: Some("touch file".to_string()),
        metadata: HashMap::new(),
    })
    .unwrap();

    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].original_cwd, "/repo");
    assert_eq!(calls[0].repo_work_dir.as_deref(), Some("/repo"));
    assert_eq!(calls[0].start_trace_id.as_deref(), Some("t_end"));
    assert_eq!(calls[0].end_trace_id.as_deref(), Some("t_end"));
    assert_eq!(calls[0].start_time_ns, 5_000);
    assert_eq!(calls[0].end_time_ns, Some(5_000));
}

#[test]
fn unresolved_cwd_call_is_available_as_candidate() {
    let (mut db, _dir) = test_db();

    db.record_start(&BashCallStart {
        original_cwd: "/workspace".to_string(),
        repo_work_dir: None,
        repo_discovery_error: Some("No git repository found".to_string()),
        session_id: "session-3".to_string(),
        tool_use_id: "tool-3".to_string(),
        agent_id: test_agent(),
        start_trace_id: "t_start".to_string(),
        started_at_ns: 1_000,
        command: Some("cd project && printf x >> src/a.rs".to_string()),
        metadata: HashMap::new(),
    })
    .unwrap();
    db.record_end(&BashCallEnd {
        original_cwd: "/workspace".to_string(),
        repo_work_dir: None,
        repo_discovery_error: Some("No git repository found".to_string()),
        session_id: "session-3".to_string(),
        tool_use_id: "tool-3".to_string(),
        agent_id: test_agent(),
        start_trace_id: Some("t_start".to_string()),
        end_trace_id: "t_end".to_string(),
        started_at_ns: Some(1_000),
        ended_at_ns: 2_000,
        command: Some("cd project && printf x >> src/a.rs".to_string()),
        metadata: HashMap::new(),
    })
    .unwrap();

    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].original_cwd, "/workspace");
    assert_eq!(calls[0].repo_work_dir, None);
    assert_eq!(
        calls[0].repo_discovery_error.as_deref(),
        Some("No git repository found")
    );

    let candidates = db.candidates_near_timestamps(&[1_500], 1_000).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].tool_use_id, "tool-3");
    assert_eq!(candidates[0].repo_work_dir, None);
}

#[test]
fn candidate_query_filters_by_window() {
    let (mut db, _dir) = test_db();
    for (repo_work_dir, tool_use_id, start, end) in [
        ("/repo", "near-before", 1_000_u128, 2_000_u128),
        ("/other-repo", "near-after", 8_000, 9_000),
        ("/repo", "outside", 20_000, 21_000),
    ] {
        db.record_start(&BashCallStart {
            original_cwd: repo_work_dir.to_string(),
            repo_work_dir: Some(repo_work_dir.to_string()),
            repo_discovery_error: None,
            session_id: "session".to_string(),
            tool_use_id: tool_use_id.to_string(),
            agent_id: test_agent(),
            start_trace_id: format!("t_{}", tool_use_id),
            started_at_ns: start,
            command: None,
            metadata: HashMap::new(),
        })
        .unwrap();
        db.record_end(&BashCallEnd {
            original_cwd: repo_work_dir.to_string(),
            repo_work_dir: Some(repo_work_dir.to_string()),
            repo_discovery_error: None,
            session_id: "session".to_string(),
            tool_use_id: tool_use_id.to_string(),
            agent_id: test_agent(),
            start_trace_id: Some(format!("t_{}", tool_use_id)),
            end_trace_id: format!("t_end_{}", tool_use_id),
            started_at_ns: Some(start),
            ended_at_ns: end,
            command: None,
            metadata: HashMap::new(),
        })
        .unwrap();
    }

    let calls = db.candidates_near_timestamps(&[5_000], 3_000).unwrap();
    let ids: Vec<_> = calls.iter().map(|c| c.tool_use_id.as_str()).collect();
    assert_eq!(ids, vec!["near-before", "near-after"]);
}

#[test]
fn fallback_database_at_file_has_schema() {
    let dir = tempfile::tempdir().unwrap();
    let db = BashHistoryDatabase::fallback_database_at(&dir.path().join("fallback.db")).unwrap();

    let calls = db.all_calls_for_test().unwrap();
    assert!(calls.is_empty());
}

#[test]
fn fallback_database_returns_error_when_file_path_fails() {
    let dir = tempfile::tempdir().unwrap();
    match BashHistoryDatabase::fallback_database_at(dir.path()) {
        Ok(_) => panic!("fallback database unexpectedly opened a directory path"),
        Err(err) => assert!(
            err.to_string()
                .contains("Failed to initialize fallback bash history database")
        ),
    }
}

#[test]
fn retention_prunes_rows_older_than_thirty_days() {
    let (mut db, _dir) = test_db();
    db.record_start(&BashCallStart {
        original_cwd: "/repo".to_string(),
        repo_work_dir: Some("/repo".to_string()),
        repo_discovery_error: None,
        session_id: "old".to_string(),
        tool_use_id: "old-tool".to_string(),
        agent_id: test_agent(),
        start_trace_id: "t_old".to_string(),
        started_at_ns: 1_000,
        command: None,
        metadata: HashMap::new(),
    })
    .unwrap();
    db.record_start(&BashCallStart {
        original_cwd: "/repo".to_string(),
        repo_work_dir: Some("/repo".to_string()),
        repo_discovery_error: None,
        session_id: "new".to_string(),
        tool_use_id: "new-tool".to_string(),
        agent_id: test_agent(),
        start_trace_id: "t_new".to_string(),
        started_at_ns: 2_000,
        command: None,
        metadata: HashMap::new(),
    })
    .unwrap();

    let now = 10_000_000;
    db.conn
        .execute(
            "UPDATE bash_checkpoint_calls SET updated_at = ?1 WHERE session_id = 'old'",
            params![(now - RETENTION_SECS - 1) as i64],
        )
        .unwrap();
    db.conn
        .execute(
            "UPDATE bash_checkpoint_calls SET updated_at = ?1 WHERE session_id = 'new'",
            params![(now - RETENTION_SECS + 1) as i64],
        )
        .unwrap();

    db.prune_old_calls(now).unwrap();
    let calls = db.all_calls_for_test().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].session_id, "new");
}
