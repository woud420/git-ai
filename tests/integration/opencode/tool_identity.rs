use super::{ParsedHookEvent, fixture_path, json, parse_and_enrich_opencode};

#[test]
fn test_opencode_transcript_ids_extracted_from_fixture() {
    use chrono::{DateTime, Utc};
    use git_ai::model::stream_watermark::TimestampWatermark;
    use git_ai::operations::streams::agent::Agent;
    use git_ai::operations::streams::agents::OpenCodeAgent;

    let fixture = fixture_path("opencode-sqlite/opencode.db");
    let agent = OpenCodeAgent::new();
    let watermark = Box::new(TimestampWatermark::new(DateTime::<Utc>::UNIX_EPOCH));
    let batch = agent
        .read_incremental(&fixture, watermark, "test-session-123")
        .unwrap();

    assert_eq!(batch.events.len(), 2, "Fixture has 2 messages");

    // Event 0: user message — has id, no parentID in data, no tool parts with callID
    let (eid, pid, tid) = agent.extract_event_ids(&batch.events[0]);
    assert_eq!(eid, Some("msg-user-sql-001".to_string()));
    assert_eq!(pid, None);
    assert_eq!(tid, None);

    // Event 1: assistant message — has id, parentID points to user msg, tool part has callID
    let (eid, pid, tid) = agent.extract_event_ids(&batch.events[1]);
    assert_eq!(eid, Some("msg-assistant-sql-001".to_string()));
    assert_eq!(pid, Some("msg-user-sql-001".to_string()));
    assert_eq!(tid, Some("call-sql-001".to_string()));
}

#[test]
fn test_opencode_tool_use_id_matches_hook_and_transcript() {
    use chrono::{DateTime, Utc};
    use git_ai::model::stream_watermark::TimestampWatermark;
    use git_ai::operations::streams::agent::Agent;
    use git_ai::operations::streams::agents::OpenCodeAgent;

    let fixture = fixture_path("opencode-sqlite/opencode.db");
    let agent = OpenCodeAgent::new();
    let watermark = Box::new(TimestampWatermark::new(DateTime::<Utc>::UNIX_EPOCH));
    let batch = agent
        .read_incremental(&fixture, watermark, "test-session-123")
        .unwrap();

    let assistant_event = &batch.events[1];
    let (_, _, tool_use_id_from_transcript) = agent.extract_event_ids(assistant_event);

    let hook_tool_use_id = "call-sql-001";
    assert_eq!(
        tool_use_id_from_transcript,
        Some(hook_tool_use_id.to_string()),
        "Transcript callID must match what the hook sends as tool_use_id"
    );
}

#[test]
#[serial_test::serial]
fn test_opencode_checkpoint_tool_use_id_matches_transcript_callid() {
    use crate::repos::test_repo::TestRepo;
    use chrono::{DateTime, Utc};
    use git_ai::model::stream_watermark::TimestampWatermark;
    use git_ai::operations::streams::agent::Agent;
    use git_ai::operations::streams::agents::OpenCodeAgent;

    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
    });

    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("index.ts");
    std::fs::write(&file_path, "// initial\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    let temp_storage = tempfile::tempdir().unwrap();
    let storage_path = temp_storage.path();
    let fixture_db = fixture_path("opencode-sqlite/opencode.db");
    std::fs::copy(&fixture_db, storage_path.join("opencode.db")).unwrap();

    unsafe {
        std::env::set_var(
            "GIT_AI_OPENCODE_STORAGE_PATH",
            storage_path.to_str().unwrap(),
        );
    }

    let pre_hook_input = json!({
        "hook_event_name": "PreToolUse",
        "session_id": "test-session-123",
        "tool_use_id": "call-sql-001",
        "cwd": repo_root.to_string_lossy().to_string(),
        "tool_name": "edit",
        "tool_input": {
            "filePath": file_path.to_string_lossy().to_string()
        }
    })
    .to_string();

    repo.checkpoint_with_hook_input("opencode", &pre_hook_input)
        .unwrap();

    std::fs::write(&file_path, "// initial\n// AI edit\n").unwrap();

    let post_hook_input = json!({
        "hook_event_name": "PostToolUse",
        "session_id": "test-session-123",
        "tool_use_id": "call-sql-001",
        "cwd": repo_root.to_string_lossy().to_string(),
        "tool_name": "edit",
        "tool_input": {
            "filePath": file_path.to_string_lossy().to_string()
        }
    })
    .to_string();

    repo.checkpoint_with_hook_input("opencode", &post_hook_input)
        .unwrap();

    unsafe {
        std::env::remove_var("GIT_AI_OPENCODE_STORAGE_PATH");
    }

    let commit = repo.stage_all_and_commit("Add AI line").unwrap();

    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Should have at least one session record"
    );

    let agent = OpenCodeAgent::new();
    let watermark = Box::new(TimestampWatermark::new(DateTime::<Utc>::UNIX_EPOCH));
    let batch = agent
        .read_incremental(&fixture_db, watermark, "test-session-123")
        .unwrap();

    let (_, _, tid) = agent.extract_event_ids(&batch.events[1]);
    assert_eq!(
        tid,
        Some("call-sql-001".to_string()),
        "Transcript callID must equal the tool_use_id sent in the checkpoint hook"
    );
}

#[test]
#[serial_test::serial]
fn test_opencode_checkpoint_sets_parent_session_id_from_db() {
    let temp_storage = tempfile::tempdir().unwrap();
    let storage_path = temp_storage.path();
    let fixture_db = fixture_path("opencode-sqlite/opencode.db");
    std::fs::copy(&fixture_db, storage_path.join("opencode.db")).unwrap();

    unsafe {
        std::env::set_var(
            "GIT_AI_OPENCODE_STORAGE_PATH",
            storage_path.to_str().unwrap(),
        );
    }

    let hook_input = json!({
        "hook_event_name": "PostToolUse",
        "session_id": "test-session-123",
        "cwd": "/Users/test/project",
        "tool_name": "edit",
        "tool_use_id": "call-sql-001",
        "tool_input": {
            "filePath": "/Users/test/project/index.ts"
        }
    })
    .to_string();

    let events = parse_and_enrich_opencode(&hook_input).unwrap();

    unsafe {
        std::env::remove_var("GIT_AI_OPENCODE_STORAGE_PATH");
    }

    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            let ts = e
                .stream_source
                .as_ref()
                .expect("should have transcript source");
            assert_eq!(
                ts.external_parent_session_id,
                Some("parent-session-456".to_string()),
                "OpenCode checkpoint should look up parent_id from session table"
            );
            assert_eq!(ts.external_session_id, "test-session-123",);
        }
        _ => panic!("Expected PostFileEdit"),
    }
}
