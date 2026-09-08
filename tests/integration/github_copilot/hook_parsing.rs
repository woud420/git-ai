use super::{
    ByteOffsetWatermark, CopilotAgent, ParsedHookEvent, RecordIndexWatermark, ensure_clean_env,
    fixture_path, json, parse_copilot, read_jsonl_fixture,
};
use git_ai::operations::streams::agent::Agent;
use std::io::Write;

#[test]
#[serial_test::serial(copilot_env)]
fn test_copilot_session_json_raw_event_fidelity() {
    ensure_clean_env();
    let fixture = fixture_path("copilot_session_simple.json");
    let agent = CopilotAgent::new();
    let watermark = Box::new(RecordIndexWatermark::new(0));
    let result = agent
        .read_incremental(fixture.as_path(), watermark, "test")
        .expect("Should parse copilot session JSON");

    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture).unwrap()).unwrap();
    let expected: Vec<serde_json::Value> = parsed["requests"].as_array().unwrap().clone();

    assert_eq!(result.events, expected);
}

#[test]
#[serial_test::serial(copilot_env)]
fn test_copilot_event_stream_raw_event_fidelity() {
    ensure_clean_env();
    let fixture = fixture_path("copilot_session_event_stream.jsonl");
    let agent = CopilotAgent::new();
    let watermark = Box::new(ByteOffsetWatermark::new(0));
    let result = agent
        .read_incremental(fixture.as_path(), watermark, "test")
        .expect("Should parse copilot event stream JSONL");

    let expected = read_jsonl_fixture(&fixture).unwrap();

    assert_eq!(result.events.len(), expected.len());
    assert_eq!(result.events, expected);
}

#[test]
#[serial_test::serial(copilot_env)]
fn test_copilot_returns_empty_transcript_in_codespaces() {
    let original_codespaces = std::env::var("CODESPACES").ok();
    unsafe {
        std::env::set_var("CODESPACES", "true");
    }

    let fixture = fixture_path("copilot_session_simple.json");
    let agent = CopilotAgent::new();
    let watermark = Box::new(RecordIndexWatermark::new(0));
    let result = agent.read_incremental(fixture.as_path(), watermark, "test");
    assert!(result.is_ok());
    let batch = result.unwrap();
    assert!(batch.events.is_empty());

    unsafe {
        if let Some(original) = original_codespaces {
            std::env::set_var("CODESPACES", original);
        } else {
            std::env::remove_var("CODESPACES");
        }
    }
}

#[test]
#[serial_test::serial(copilot_env)]
fn test_copilot_returns_empty_transcript_in_remote_containers() {
    let original = std::env::var("REMOTE_CONTAINERS").ok();
    unsafe {
        std::env::set_var("REMOTE_CONTAINERS", "true");
    }

    let fixture = fixture_path("copilot_session_simple.json");
    let agent = CopilotAgent::new();
    let watermark = Box::new(RecordIndexWatermark::new(0));
    let result = agent.read_incremental(fixture.as_path(), watermark, "test");
    assert!(result.is_ok());
    let batch = result.unwrap();
    assert!(batch.events.is_empty());

    unsafe {
        if let Some(orig) = original {
            std::env::set_var("REMOTE_CONTAINERS", orig);
        } else {
            std::env::remove_var("REMOTE_CONTAINERS");
        }
    }
}

// ============================================================================
// Tests for before_edit / after_edit logic
// ============================================================================

#[test]
fn test_copilot_preset_before_edit_human_checkpoint_snake_case() {
    let hook_input = json!({
        "hook_event_name": "before_edit",
        "workspace_folder": "/Users/test/project",
        "will_edit_filepaths": ["/Users/test/project/file.ts"],
        "dirty_files": { "/Users/test/project/file.ts": "console.log('hello');" }
    })
    .to_string();

    let events = parse_copilot(&hook_input).expect("Should succeed");
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert!(!e.file_paths.is_empty());
            assert!(e.dirty_files.is_some());
            let dirty_files = e.dirty_files.as_ref().unwrap();
            assert_eq!(dirty_files.len(), 1);
            assert!(dirty_files.values().any(|v| v.contains("hello")));
            assert_eq!(e.context.agent_id.tool, "github-copilot");
        }
        _ => panic!("Expected PreFileEdit for before_edit"),
    }
}

#[test]
fn test_copilot_preset_before_edit_human_checkpoint_camel_case() {
    let hook_input = json!({
        "hook_event_name": "before_edit",
        "workspaceFolder": "/Users/test/project",
        "will_edit_filepaths": ["/Users/test/project/file.ts"],
        "dirtyFiles": { "/Users/test/project/file.ts": "console.log('hello');" }
    })
    .to_string();

    let events = parse_copilot(&hook_input).expect("Should succeed");
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert!(!e.file_paths.is_empty());
            assert!(e.dirty_files.is_some());
        }
        _ => panic!("Expected PreFileEdit for before_edit"),
    }
}

#[test]
fn test_copilot_preset_before_edit_requires_will_edit_filepaths() {
    let hook_input = json!({
        "hook_event_name": "before_edit",
        "workspace_folder": "/Users/test/project",
        "dirty_files": {}
    })
    .to_string();

    let result = parse_copilot(&hook_input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("will_edit_filepaths is required")
    );
}

#[test]
fn test_copilot_preset_before_edit_requires_non_empty_filepaths() {
    let hook_input = json!({
        "hook_event_name": "before_edit",
        "workspace_folder": "/Users/test/project",
        "will_edit_filepaths": [],
        "dirty_files": {}
    })
    .to_string();

    let result = parse_copilot(&hook_input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("will_edit_filepaths cannot be empty")
    );
}

#[test]
fn test_copilot_preset_after_edit_requires_session_id() {
    let hook_input = json!({
        "hook_event_name": "after_edit",
        "workspace_folder": "/Users/test/project",
        "dirty_files": {}
    })
    .to_string();

    let result = parse_copilot(&hook_input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("chat_session_path or chatSessionPath not found")
    );
}

#[test]
fn test_copilot_preset_after_edit_requires_session_id_camel_case() {
    let hook_input = json!({
        "hook_event_name": "after_edit",
        "workspaceFolder": "/Users/test/project",
        "dirtyFiles": {}
    })
    .to_string();

    let result = parse_copilot(&hook_input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("chat_session_path or chatSessionPath not found")
    );
}

#[test]
fn test_copilot_preset_invalid_hook_event_name() {
    let hook_input = json!({
        "hook_event_name": "invalid_event",
        "workspace_folder": "/Users/test/project"
    })
    .to_string();

    let result = parse_copilot(&hook_input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid hook_event_name")
    );
}

#[test]
fn test_copilot_preset_before_edit_multiple_files_snake_case() {
    let hook_input = json!({
        "hook_event_name": "before_edit",
        "workspace_folder": "/Users/test/project",
        "will_edit_filepaths": ["/Users/test/project/file1.ts", "/Users/test/project/file2.ts", "/Users/test/project/file3.ts"],
        "dirty_files": { "/Users/test/project/file1.ts": "content1", "/Users/test/project/file2.ts": "content2" }
    }).to_string();

    let events = parse_copilot(&hook_input).expect("Should succeed");
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.file_paths.len(), 3);
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_copilot_preset_before_edit_multiple_files_camel_case() {
    let hook_input = json!({
        "hook_event_name": "before_edit",
        "workspaceFolder": "/Users/test/project",
        "will_edit_filepaths": ["/Users/test/project/file1.ts", "/Users/test/project/file2.ts", "/Users/test/project/file3.ts"],
        "dirtyFiles": { "/Users/test/project/file1.ts": "content1", "/Users/test/project/file2.ts": "content2" }
    }).to_string();

    let events = parse_copilot(&hook_input).expect("Should succeed");
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.file_paths.len(), 3);
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_copilot_preset_after_edit_camel_case() {
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    temp_file
        .write_all(r#"{"requests": []}"#.as_bytes())
        .unwrap();
    let temp_path = temp_file.path().to_str().unwrap().to_string();

    let hook_input = json!({
        "hook_event_name": "after_edit",
        "workspaceFolder": "/Users/test/project",
        "chatSessionPath": temp_path,
        "sessionId": "test-session-123",
        "edited_filepaths": ["/Users/test/project/file.ts"],
        "dirtyFiles": { "/Users/test/project/file.ts": "console.log('hello');" }
    })
    .to_string();

    let events = parse_copilot(&hook_input).expect("Should succeed");
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.id, "test-session-123");
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert!(!e.file_paths.is_empty());
            assert!(e.dirty_files.is_some());
        }
        _ => panic!("Expected PostFileEdit for after_edit"),
    }
}

#[test]
fn test_copilot_preset_after_edit_snake_case() {
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    temp_file
        .write_all(r#"{"requests": []}"#.as_bytes())
        .unwrap();
    let temp_path = temp_file.path().to_str().unwrap().to_string();

    let hook_input = json!({
        "hook_event_name": "after_edit",
        "workspace_folder": "/Users/test/project",
        "chat_session_path": temp_path,
        "session_id": "test-session-456",
        "edited_filepaths": ["/Users/test/project/file.ts"],
        "dirty_files": { "/Users/test/project/file.ts": "console.log('hello');" }
    })
    .to_string();

    let events = parse_copilot(&hook_input).expect("Should succeed");
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.id, "test-session-456");
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert!(!e.file_paths.is_empty());
            assert!(e.dirty_files.is_some());
        }
        _ => panic!("Expected PostFileEdit for after_edit"),
    }
}

// ============================================================================
// Tests for JSONL format support
// ============================================================================

// NOTE: copilot_session_parsing_jsonl_stub, copilot_session_parsing_jsonl_simple,
// and test_copilot_extracts_edited_filepaths_jsonl were removed because the new
// CopilotAgent API does not support the kind:0/kind:1 JSONL snapshot+patch protocol,
// and edited_filepaths are no longer returned by read_incremental.

#[test]
#[serial_test::serial(copilot_env)]
fn test_copilot_after_edit_with_jsonl_session() {
    ensure_clean_env();

    let mut temp_file = tempfile::NamedTempFile::with_suffix(".jsonl").unwrap();
    temp_file
        .write_all(r#"{"kind":0,"v":{"requests": []}}"#.as_bytes())
        .unwrap();
    let temp_path = temp_file.path().to_str().unwrap().to_string();

    let hook_input = json!({
        "hook_event_name": "after_edit",
        "workspace_folder": "/Users/test/project",
        "chat_session_path": temp_path,
        "session_id": "test-jsonl-session-789",
        "edited_filepaths": ["/Users/test/project/file.ts"],
        "dirty_files": { "/Users/test/project/file.ts": "console.log('hello');" }
    })
    .to_string();

    let events = parse_copilot(&hook_input).expect("Should succeed");
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.id, "test-jsonl-session-789");
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert!(!e.file_paths.is_empty());
            assert!(e.dirty_files.is_some());
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_copilot_preset_cli_non_edit_tool_is_filtered() {
    let hook_input = json!({
        "hookEventName": "PreToolUse",
        "cwd": "/Users/test/project",
        "toolName": "view",
        "toolInput": { "path": "/Users/test/project/file.ts" },
        "sessionId": "copilot-session-view"
    })
    .to_string();

    let result = parse_copilot(&hook_input);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("non-edit tool"),
        "Expected non-edit tool error, got: {}",
        err_msg
    );
}
