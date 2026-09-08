use super::{
    Arc, ByteOffsetWatermark, ClaudeAgent, EventAttributes, ExpectedLineExt, MetricEvent,
    OpenCodeAgent, PathBuf, SessionEventValues, StreamFormat, StreamRecord, StreamsDatabase,
    TempDir, TestRepo, TimestampWatermark, WatermarkType, fixture_path, transcript_fixture_path,
};
use git_ai::metrics::PosEncoded;
use git_ai::operations::streams::agent::Agent;
use std::fs;

#[test]
fn test_full_pipeline_claude_session_ids_flow_through() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("transcripts.db");
    let db = Arc::new(StreamsDatabase::open(&db_path).unwrap());

    let fixture = transcript_fixture_path("claude_with_ids.jsonl");
    let now = chrono::Utc::now().timestamp();

    let session = StreamRecord {
        session_id: "sess-parent-abc".to_string(),
        stream_kind: "transcript".to_string(),
        tool: "claude".to_string(),
        stream_path: fixture.display().to_string(),
        stream_format: StreamFormat::ClaudeJsonl,
        watermark_type: WatermarkType::ByteOffset,
        watermark_value: "0".to_string(),
        external_session_id: "sess-parent-abc".to_string(),
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

    let retrieved = db
        .get_stream(
            "sess-parent-abc",
            "transcript",
            &fixture.display().to_string(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.external_session_id, "sess-parent-abc".to_string());
    assert_eq!(retrieved.external_parent_session_id, None);

    let agent = ClaudeAgent::new();
    let watermark = Box::new(ByteOffsetWatermark::new(0));
    let batch = agent
        .read_incremental(
            &PathBuf::from(&retrieved.stream_path),
            watermark,
            &retrieved.session_id,
        )
        .unwrap();

    let attrs_sparse = EventAttributes::with_version("test")
        .session_id(retrieved.session_id.clone())
        .external_session_id(retrieved.external_session_id.clone())
        .external_parent_session_id_opt(retrieved.external_parent_session_id.clone())
        .to_sparse();

    let metric_events: Vec<MetricEvent> = batch
        .events
        .into_iter()
        .map(|raw_event| {
            let (eid, pid, tid) = agent.extract_event_ids(&raw_event);
            MetricEvent::from_values(
                SessionEventValues::with_ids(raw_event, eid, pid, tid),
                attrs_sparse.clone(),
            )
        })
        .collect();

    assert_eq!(metric_events.len(), 5);

    let attrs = EventAttributes::from_sparse(&metric_events[0].attrs);
    assert_eq!(attrs.session_id, Some(Some("sess-parent-abc".to_string())));
    assert_eq!(
        attrs.external_session_id,
        Some(Some("sess-parent-abc".to_string()))
    );
    assert_eq!(attrs.external_parent_session_id, None);

    let values = SessionEventValues::from_sparse(&metric_events[2].values);
    assert_eq!(
        values.external_event_id,
        Some("ccc33333-3333-3333-3333-333333333333".to_string())
    );
    assert_eq!(
        values.external_parent_event_id,
        Some("bbb22222-2222-2222-2222-222222222222".to_string())
    );
    assert_eq!(
        values.external_tool_use_id,
        Some("toolu_01AbCdEfGhIjKlMnOp".to_string())
    );
}

#[test]
fn test_full_pipeline_opencode_session_ids_flow_through() {
    use chrono::{DateTime, Utc};

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("transcripts.db");
    let db = Arc::new(StreamsDatabase::open(&db_path).unwrap());

    let fixture = fixture_path("opencode-sqlite/opencode.db");
    let now = chrono::Utc::now().timestamp();

    let session = StreamRecord {
        session_id: "test-session-123".to_string(),
        stream_kind: "transcript".to_string(),
        tool: "opencode".to_string(),
        stream_path: fixture.display().to_string(),
        stream_format: StreamFormat::OpenCodeSqlite,
        watermark_type: WatermarkType::Timestamp,
        watermark_value: DateTime::<Utc>::UNIX_EPOCH.to_rfc3339(),
        external_session_id: "test-session-123".to_string(),
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

    let agent = OpenCodeAgent::new();
    let watermark = Box::new(TimestampWatermark::new(DateTime::<Utc>::UNIX_EPOCH));
    let batch = agent
        .read_incremental(
            &PathBuf::from(&session.stream_path),
            watermark,
            &session.session_id,
        )
        .unwrap();

    let attrs_sparse = EventAttributes::with_version("test")
        .session_id(session.session_id.clone())
        .external_session_id(session.external_session_id.clone())
        .external_parent_session_id_opt(session.external_parent_session_id.clone())
        .to_sparse();

    let metric_events: Vec<MetricEvent> = batch
        .events
        .into_iter()
        .map(|raw_event| {
            let (eid, pid, tid) = agent.extract_event_ids(&raw_event);
            MetricEvent::from_values(
                SessionEventValues::with_ids(raw_event, eid, pid, tid),
                attrs_sparse.clone(),
            )
        })
        .collect();

    assert_eq!(metric_events.len(), 2);

    let values = SessionEventValues::from_sparse(&metric_events[1].values);
    assert_eq!(
        values.external_event_id,
        Some("msg-assistant-sql-001".to_string())
    );
    assert_eq!(
        values.external_parent_event_id,
        Some("msg-user-sql-001".to_string())
    );
    assert_eq!(
        values.external_tool_use_id,
        Some("call-sql-001".to_string())
    );
}

#[test]
fn test_subagent_session_record_has_parent_link() {
    use git_ai::operations::streams::agents::claude::ClaudeAgent as ClaudeAgentImpl;

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("transcripts.db");
    let db = StreamsDatabase::open(&db_path).unwrap();

    let subagent_path = PathBuf::from(
        "/home/user/.claude/projects/proj/sess-parent-abc/subagents/agent-a1b2c3d4e5f6.jsonl",
    );
    let parent_id = ClaudeAgentImpl::detect_subagent_parent(&subagent_path);
    assert_eq!(parent_id, Some("sess-parent-abc".to_string()));

    let now = chrono::Utc::now().timestamp();
    let session = StreamRecord {
        session_id: "agent-a1b2c3d4e5f6".to_string(),
        stream_kind: "transcript".to_string(),
        tool: "claude".to_string(),
        stream_path: subagent_path.display().to_string(),
        stream_format: StreamFormat::ClaudeJsonl,
        watermark_type: WatermarkType::ByteOffset,
        watermark_value: "0".to_string(),
        external_session_id: "agent-a1b2c3d4e5f6".to_string(),
        external_parent_session_id: parent_id.clone(),
        first_seen_at: now,
        last_processed_at: 0,
        last_known_size: 0,
        last_modified: None,
        processing_errors: 0,
        last_error: None,
        repo_work_dir: None,
    };
    db.insert_stream(&session).unwrap();

    let retrieved = db
        .get_stream(
            "agent-a1b2c3d4e5f6",
            "transcript",
            &subagent_path.display().to_string(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        retrieved.external_session_id,
        "agent-a1b2c3d4e5f6".to_string()
    );
    assert_eq!(
        retrieved.external_parent_session_id,
        Some("sess-parent-abc".to_string())
    );

    let attrs = EventAttributes::with_version("test")
        .session_id(retrieved.session_id.clone())
        .external_session_id(retrieved.external_session_id.clone())
        .external_parent_session_id_opt(retrieved.external_parent_session_id.clone())
        .to_sparse();

    let restored = EventAttributes::from_sparse(&attrs);
    assert_eq!(
        restored.external_session_id,
        Some(Some("agent-a1b2c3d4e5f6".to_string()))
    );
    assert_eq!(
        restored.external_parent_session_id,
        Some(Some("sess-parent-abc".to_string()))
    );
}

// Regression coverage for ENG-324 and upstream git-ai-project/git-ai#2204 and
// git-ai-project/git-ai#2223.
#[test]
fn codex_subagent_rollouts_sharing_hook_session_register_distinct_streams() {
    let repo = TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.feature_flags = Some(serde_json::json!({"transcript_sweep": false}));
    });
    let parent_id = "01a00000-0000-7000-8000-0000000000aa";
    let child_ids = [
        "01a00000-0000-7000-8000-0000000000b1",
        "01a00000-0000-7000-8000-0000000000b2",
    ];
    let file_names = ["child-a.txt", "child-b.txt"];

    for file_name in file_names {
        fs::write(repo.path().join(file_name), "base\n").unwrap();
    }
    repo.stage_all_and_commit("seed Codex subagent files")
        .unwrap();
    for file_name in file_names {
        let mut file = repo.filename(file_name);
        file.assert_committed_lines(crate::lines!["base".unattributed_human()]);
    }

    // Sweeping is disabled for this dedicated daemon, so registration below
    // can only come from the checkpoint path under test.
    let rollout_dir = repo.daemon_home_path().join(".codex/sessions/2026/08/31");
    fs::create_dir_all(&rollout_dir).unwrap();
    let mut rollout_paths = Vec::new();

    for ((child_id, file_name), index) in child_ids.into_iter().zip(file_names).zip(1..) {
        let rollout = rollout_dir.join(format!(
            "rollout-2026-08-31T15-00-0{index}-{child_id}.jsonl"
        ));
        fs::write(
            &rollout,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{child_id}\",\"forked_from_id\":\"{parent_id}\",\"thread_source\":\"subagent\",\"model\":\"gpt-5.1-codex\"}}}}\n"
            ),
        )
        .unwrap();
        rollout_paths.push(rollout.clone());

        let file_path = repo.path().join(file_name);
        let tool_use_id = format!("tu-child-{index}");
        for hook_event_name in ["PreToolUse", "PostToolUse"] {
            let hook_input = serde_json::json!({
                "cwd": repo.canonical_path().to_string_lossy(),
                "hook_event_name": hook_event_name,
                "tool_name": "apply_patch",
                "tool_use_id": tool_use_id,
                "session_id": parent_id,
                "transcript_path": rollout.to_string_lossy(),
                "tool_input": {
                    "patch": format!(
                        "*** Update File: {}\n@@ base\n+child {index} edit\n",
                        file_path.to_string_lossy()
                    )
                }
            })
            .to_string();
            repo.git_ai(&["checkpoint", "codex", "--hook-input", &hook_input])
                .expect("Codex checkpoint should succeed");

            if hook_event_name == "PreToolUse" {
                fs::write(&file_path, format!("base\nchild {index} edit\n")).unwrap();
            }
        }
    }

    repo.sync_daemon();
    repo.git_ai(&["await", "--timeout", "60"])
        .expect("stream registration should drain");

    let db_path = repo
        .daemon_home_path()
        .join(".git-ai")
        .join("internal")
        .join("transcripts-db");
    let db = StreamsDatabase::open(&db_path).unwrap();
    let canonical_rollouts: Vec<String> = rollout_paths
        .iter()
        .map(|path| path.canonicalize().unwrap().display().to_string())
        .collect();
    let mut rows: Vec<StreamRecord> = db
        .all_streams()
        .unwrap()
        .into_iter()
        .filter(|row| canonical_rollouts.contains(&row.stream_path))
        .collect();
    rows.sort_by(|left, right| left.external_session_id.cmp(&right.external_session_id));

    assert_eq!(
        rows.len(),
        2,
        "each rollout must have exactly one stream row"
    );
    for (row, child_id) in rows.iter().zip(child_ids) {
        assert_eq!(row.external_session_id, child_id);
        assert_eq!(
            row.session_id,
            git_ai::model::authorship_log_serialization::generate_session_id(child_id, "codex")
        );
        assert_eq!(row.external_parent_session_id.as_deref(), Some(parent_id));
    }

    let commit = repo
        .stage_all_and_commit("commit distinct Codex subagent edits")
        .unwrap();
    for (file_name, index) in file_names.into_iter().zip(1..) {
        let mut file = repo.filename(file_name);
        file.assert_committed_lines(crate::lines![
            "base".unattributed_human(),
            format!("child {index} edit").ai(),
        ]);
    }
    assert_eq!(
        commit.authorship_log.metadata.sessions.len(),
        1,
        "both child rollouts must retain one logical checkpoint session"
    );
    let session = commit
        .authorship_log
        .metadata
        .sessions
        .values()
        .next()
        .expect("logical checkpoint session should exist");
    assert_eq!(session.agent_id.tool, "codex");
    assert_eq!(session.agent_id.id, parent_id);
}
