use super::{
    ByteOffsetWatermark, CodexAgent, ParsedHookEvent, fixture_path, fs, json, parse_codex,
    read_jsonl_fixture,
};
use git_ai::operations::streams::agent::Agent;

#[test]
fn test_codex_raw_event_fidelity() {
    let fixture = fixture_path("codex-session-simple.jsonl");
    let agent = CodexAgent::new();
    let watermark = Box::new(ByteOffsetWatermark::new(0));
    let result = agent
        .read_incremental(fixture.as_path(), watermark, "test")
        .expect("Should parse codex JSONL");

    let expected = read_jsonl_fixture(&fixture).unwrap();

    assert_eq!(result.events.len(), expected.len());
    assert_eq!(result.events, expected);
}

#[test]
fn test_codex_preset_structured_hook_input() {
    let fixture = fixture_path("codex-session-simple.jsonl");
    let hook_input = json!({
        "session_id": "session-abc-123",
        "cwd": "/Users/test/projects/git-ai",
        "hook_event_name": "PostToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-1",
        "triggered_at": "2026-02-11T05:53:33Z",
        "hook_event": {
            "event_type": "after_agent",
            "thread_id": "thread-xyz-999",
            "turn_id": "turn-2",
            "input_messages": ["Refactor src/main.rs"],
            "last_assistant_message": "Done."
        },
        "transcript_path": fixture.to_str().unwrap()
    })
    .to_string();

    let events = parse_codex(&hook_input).expect("Codex preset should run");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "codex");
            assert_eq!(
                e.context.external_session_id, "session-abc-123",
                "session_id should be preferred when present"
            );
            assert_eq!(
                e.context.cwd.to_string_lossy(),
                "/Users/test/projects/git-ai"
            );
            assert!(e.stream_source.is_some());
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

// Regression coverage for ENG-324 and upstream git-ai-project/git-ai#2204.
#[test]
fn test_codex_subagent_stream_identity_comes_from_rollout_filename() {
    let parent_id = "01a00000-0000-7000-8000-0000000000aa";
    let child_id = "01a00000-0000-7000-8000-0000000000b1";
    let temp = tempfile::tempdir().unwrap();
    let rollout = temp
        .path()
        .join(format!("rollout-2026-08-31T15-00-00-{child_id}.jsonl"));
    fs::write(
        &rollout,
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{child_id}\",\"forked_from_id\":\"{parent_id}\",\"thread_source\":\"subagent\"}}}}\n"
        ),
    )
    .unwrap();

    let hook_input = json!({
        "session_id": parent_id,
        "cwd": "/tmp/test-project",
        "hook_event_name": "PostToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-subagent",
        "transcript_path": rollout,
        "tool_input": {
            "patch": "*** Update File: /tmp/test-project/src/lib.rs\n+child edit\n"
        }
    })
    .to_string();

    let events = parse_codex(&hook_input).expect("Codex subagent hook should parse");
    let ParsedHookEvent::PostFileEdit(event) = &events[0] else {
        panic!("expected PostFileEdit");
    };
    let source = event.stream_source.as_ref().expect("stream source");

    assert_eq!(event.context.external_session_id, parent_id);
    assert_eq!(event.context.agent_id.id, parent_id);
    assert_eq!(source.external_session_id, child_id);
    assert_eq!(
        source.session_id,
        git_ai::model::authorship_log_serialization::generate_session_id(child_id, "codex")
    );
    assert_eq!(
        source.external_parent_session_id.as_deref(),
        Some(parent_id)
    );
}

#[test]
fn test_codex_plain_rollout_keeps_existing_session_identity() {
    let session_id = "01a00000-0000-7000-8000-0000000000cc";
    let temp = tempfile::tempdir().unwrap();
    let rollout = temp
        .path()
        .join(format!("rollout-2026-08-31T15-00-00-{session_id}.jsonl"));
    fs::write(
        &rollout,
        format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\"}}}}\n"),
    )
    .unwrap();

    let hook_input = json!({
        "session_id": session_id,
        "cwd": "/tmp/test-project",
        "hook_event_name": "PostToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-plain",
        "transcript_path": rollout,
        "tool_input": {
            "patch": "*** Update File: /tmp/test-project/src/lib.rs\n+plain edit\n"
        }
    })
    .to_string();

    let events = parse_codex(&hook_input).expect("plain Codex hook should parse");
    let ParsedHookEvent::PostFileEdit(event) = &events[0] else {
        panic!("expected PostFileEdit");
    };
    let source = event.stream_source.as_ref().expect("stream source");

    assert_eq!(event.context.external_session_id, session_id);
    assert_eq!(source.external_session_id, session_id);
    assert_eq!(source.external_parent_session_id, None);
}

#[test]
fn test_codex_rollout_identity_is_stable_across_equivalent_path_spellings() {
    let child_id = "01a00000-0000-7000-8000-0000000000b1";
    let relative_suffix =
        format!("sessions/2026/08/31/rollout-2026-08-31T15-00-00-{child_id}.jsonl");

    for path in [
        std::path::PathBuf::from("/var/folders/example").join(&relative_suffix),
        std::path::PathBuf::from("/private/var/folders/example").join(&relative_suffix),
    ] {
        assert_eq!(
            CodexAgent::external_session_id_from_rollout_path(&path).as_deref(),
            Some(child_id)
        );
    }
    assert_eq!(
        CodexAgent::external_session_id_from_rollout_path(std::path::Path::new("short.jsonl")),
        None
    );
    assert_eq!(
        CodexAgent::external_session_id_from_rollout_path(std::path::Path::new(
            "/var/folders/example/transcript-2026-08-31T15-00-00-01a00000-0000-7000-8000-0000000000b1.jsonl",
        )),
        None,
        "non-rollout transcript names must fall back to the hook identity"
    );
    assert_eq!(
        CodexAgent::external_session_id_from_rollout_path(std::path::Path::new(
            "/var/folders/example/rollout-2026-08-31T15-00-00-01a00000-0000-7000-8000-0000000000bz.jsonl",
        )),
        None,
        "malformed rollout UUIDs must fail closed"
    );
}

#[test]
fn test_find_rollout_path_for_session_in_home() {
    let fixture = fixture_path("codex-session-simple.jsonl");
    let temp = tempfile::tempdir().unwrap();

    let session_id = "019c4b43-1451-7af3-be4c-5576369bf1ba";
    let rollout_dir = temp.path().join("sessions/2026/02/11");
    fs::create_dir_all(&rollout_dir).unwrap();
    let rollout_path = rollout_dir.join(format!("rollout-2026-02-11T05-53-33-{session_id}.jsonl"));
    fs::copy(&fixture, &rollout_path).unwrap();

    let resolved = CodexAgent::find_rollout_path_for_session_in_home(session_id, temp.path())
        .expect("search should succeed")
        .expect("rollout should be found");

    assert_eq!(resolved, rollout_path);
}

crate::reuse_tests_in_worktree!(
    test_codex_raw_event_fidelity,
    test_codex_preset_structured_hook_input,
    test_find_rollout_path_for_session_in_home,
);
