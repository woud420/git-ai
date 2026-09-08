use super::{
    ByteOffsetWatermark, ClaudeAgent, ParsedHookEvent, fs, json, preset_error_message,
    resolve_preset,
};
use git_ai::operations::streams::agent::Agent;

// ==============================================================================
// ClaudePreset Error Cases
// ==============================================================================

#[test]
fn test_claude_preset_invalid_json() {
    let preset = resolve_preset("claude").unwrap();
    let result = preset.parse("not valid json", "t_test");

    let msg = preset_error_message(result, "Expected PresetError for invalid JSON");
    assert!(msg.contains("Invalid JSON"));
}

#[test]
fn test_claude_preset_missing_transcript_path() {
    let preset = resolve_preset("claude").unwrap();
    let hook_input = json!({
        "cwd": "/some/path",
        "hook_event_name": "PostToolUse"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    let msg = preset_error_message(result, "Expected PresetError for missing transcript_path");
    assert!(msg.contains("transcript_path not found"));
}

#[test]
fn test_claude_preset_missing_cwd() {
    let preset = resolve_preset("claude").unwrap();
    let hook_input = json!({
        "transcript_path": "tests/fixtures/example-claude-code.jsonl",
        "hook_event_name": "PostToolUse"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    let msg = preset_error_message(result, "Expected PresetError for missing cwd");
    assert!(msg.contains("cwd not found"));
}

#[test]
fn test_claude_preset_pretooluse_checkpoint() {
    let preset = resolve_preset("claude").unwrap();
    let hook_input = json!({
        "cwd": "/some/path",
        "hook_event_name": "PreToolUse",
        "transcript_path": "tests/fixtures/example-claude-code.jsonl",
        "tool_input": {
            "file_path": "/some/file.rs"
        }
    })
    .to_string();

    let events = preset
        .parse(&hook_input, "t_test")
        .expect("Should succeed for PreToolUse");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(
                e.file_paths,
                vec![std::path::PathBuf::from("/some/file.rs")]
            );
        }
        _ => panic!("Expected PreFileEdit for PreToolUse"),
    }
}

#[test]
fn test_claude_preset_invalid_transcript_path() {
    let preset = resolve_preset("claude").unwrap();
    let hook_input = json!({
        "cwd": "/some/path",
        "hook_event_name": "PostToolUse",
        "transcript_path": "/nonexistent/path/to/transcript.jsonl"
    })
    .to_string();

    let events = preset.parse(&hook_input, "t_test");

    // Should succeed - parse doesn't read the transcript, it just records the path
    assert!(events.is_ok());
    let events = events.unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(e.stream_source.is_some());
        }
        _ => panic!("Expected PostFileEdit for PostToolUse"),
    }
}

#[test]
fn test_claude_transcript_parsing_empty_file() {
    let temp_file = std::env::temp_dir().join("empty_claude.jsonl");
    fs::write(&temp_file, "").expect("Failed to write temp file");

    let result = ClaudeAgent::new().read_incremental(
        &temp_file,
        Box::new(ByteOffsetWatermark::new(0)),
        "test",
    );

    assert!(result.is_ok());
    let batch = result.unwrap();
    assert!(batch.events.is_empty());
    // StreamBatch no longer has a model field

    fs::remove_file(temp_file).ok();
}

#[test]
fn test_claude_transcript_parsing_malformed_json() {
    let temp_file = std::env::temp_dir().join("malformed_claude.jsonl");
    fs::write(&temp_file, "{invalid json}\n").expect("Failed to write temp file");

    let result = ClaudeAgent::new().read_incremental(
        &temp_file,
        Box::new(ByteOffsetWatermark::new(0)),
        "test",
    );

    // Malformed JSON lines are skipped, not fatal errors
    let batch = result.expect("malformed lines should be skipped, not cause errors");
    assert_eq!(batch.events.len(), 0);
    fs::remove_file(temp_file).ok();
}

#[test]
fn test_claude_transcript_parsing_with_empty_lines() {
    let temp_file = std::env::temp_dir().join("empty_lines_claude.jsonl");
    let content = r#"
{"type":"user","timestamp":"2025-01-01T00:00:00Z","message":{"content":"test"}}

{"type":"assistant","timestamp":"2025-01-01T00:00:01Z","message":{"model":"claude-3","content":[{"type":"text","text":"response"}]}}
    "#;
    fs::write(&temp_file, content).expect("Failed to write temp file");

    let result = ClaudeAgent::new().read_incremental(
        &temp_file,
        Box::new(ByteOffsetWatermark::new(0)),
        "test",
    );

    assert!(result.is_ok());
    let batch = result.unwrap();
    assert_eq!(batch.events.len(), 2);
    // Model is in the raw event data, not on StreamBatch
    let model = batch
        .events
        .iter()
        .find_map(|e| e["message"]["model"].as_str());
    assert_eq!(model, Some("claude-3"));

    fs::remove_file(temp_file).ok();
}

#[test]
fn test_claude_vscode_copilot_detection() {
    let preset = resolve_preset("claude").unwrap();
    let hook_input = json!({
        "hookEventName": "PostToolUse",
        "toolName": "copilot",
        "sessionId": "test-session",
        "cwd": "/some/path",
        "transcript_path": "/Users/test/Library/Application Support/Code/User/workspaceStorage/workspace-id/GitHub.copilot-chat/transcripts/test-session.jsonl"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    let msg = preset_error_message(
        result,
        "Expected PresetError for VS Code Copilot payload in Claude preset",
    );
    assert!(msg.contains("Skipping VS Code hook payload in Claude preset"));
}

#[test]
fn test_claude_cursor_detection() {
    let preset = resolve_preset("claude").unwrap();
    let hook_input = json!({
        "conversation_id": "cursor-session-1",
        "hook_event_name": "postToolUse",
        "tool_name": "Write",
        "tool_input": {
            "file_path": "/Users/test/project/src/main.ts"
        },
        "workspace_roots": ["/Users/test/project"],
        "transcript_path": "/Users/test/.cursor/projects/Users-test-project/agent-transcripts/cursor-session-1/cursor-session-1.jsonl",
        "cursor_version": "2.5.26"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    let msg = preset_error_message(
        result,
        "Expected PresetError for Cursor payload in Claude preset",
    );
    assert!(msg.contains("Skipping Cursor hook payload in Claude preset"));
}

// ==============================================================================
// Edge Cases - Unusual but Valid Inputs
// ==============================================================================

#[test]
fn test_claude_preset_with_tool_input_no_file_path() {
    let preset = resolve_preset("claude").unwrap();
    let hook_input = json!({
        "cwd": "/path",
        "hook_event_name": "PostToolUse",
        "transcript_path": "tests/fixtures/example-claude-code.jsonl",
        "tool_input": {
            "other_field": "value"
        }
    })
    .to_string();

    let events = preset.parse(&hook_input, "t_test").expect("Should succeed");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(e.file_paths.is_empty());
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_claude_preset_with_unicode_in_path() {
    let preset = resolve_preset("claude").unwrap();
    let hook_input = json!({
        "cwd": "/Users/测试/项目",
        "hook_event_name": "PostToolUse",
        "transcript_path": "tests/fixtures/example-claude-code.jsonl",
        "tool_input": {
            "file_path": "/Users/测试/项目/文件.rs"
        }
    })
    .to_string();

    let events = preset
        .parse(&hook_input, "t_test")
        .expect("Should handle unicode paths");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(!e.file_paths.is_empty());
            assert_eq!(
                e.file_paths[0],
                std::path::PathBuf::from("/Users/测试/项目/文件.rs")
            );
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_claude_transcript_with_tool_result_in_user_content() {
    let temp_file = std::env::temp_dir().join("claude_tool_result.jsonl");
    let content = r#"{"type":"user","timestamp":"2025-01-01T00:00:00Z","message":{"content":[{"type":"tool_result","content":"should be skipped"},{"type":"text","text":"actual user input"}]}}
{"type":"assistant","timestamp":"2025-01-01T00:00:01Z","message":{"model":"claude-3","content":[{"type":"text","text":"response"}]}}"#;
    fs::write(&temp_file, content).expect("Failed to write temp file");

    let batch = ClaudeAgent::new()
        .read_incremental(&temp_file, Box::new(ByteOffsetWatermark::new(0)), "test")
        .expect("Should parse successfully");

    // Events are raw JSONL entries. The user entry is a single event.
    let user_events: Vec<_> = batch
        .events
        .iter()
        .filter(|e| e["type"] == "user")
        .collect();
    assert_eq!(user_events.len(), 1);

    fs::remove_file(temp_file).ok();
}
