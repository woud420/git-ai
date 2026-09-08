use super::{ParsedHookEvent, json, preset_error_message, resolve_preset};

// ==============================================================================
// ContinueCliPreset Error Cases
// ==============================================================================

#[test]
fn test_continue_preset_invalid_json() {
    let preset = resolve_preset("continue-cli").unwrap();
    let result = preset.parse("not json", "t_test");

    assert!(result.is_err());
}

#[test]
fn test_continue_preset_missing_session_id() {
    let preset = resolve_preset("continue-cli").unwrap();
    let hook_input = json!({
        "transcript_path": "tests/fixtures/continue-cli-session-simple.json",
        "cwd": "/path",
        "model": "gpt-4"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    let msg = preset_error_message(result, "Expected PresetError");
    assert!(msg.contains("session_id not found"));
}

#[test]
fn test_continue_preset_missing_transcript_path() {
    let preset = resolve_preset("continue-cli").unwrap();
    let hook_input = json!({
        "session_id": "test-session",
        "cwd": "/path",
        "model": "gpt-4"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    let msg = preset_error_message(result, "Expected PresetError");
    assert!(msg.contains("transcript_path not found"));
}

#[test]
fn test_continue_preset_missing_model_defaults_to_unknown() {
    let preset = resolve_preset("continue-cli").unwrap();
    let hook_input = json!({
        "session_id": "test-session",
        "transcript_path": "tests/fixtures/continue-cli-session-simple.json",
        "cwd": "/path"
    })
    .to_string();

    let events = preset
        .parse(&hook_input, "t_test")
        .expect("Should succeed with default model");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.model, "unknown");
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_continue_preset_pretooluse_checkpoint() {
    let preset = resolve_preset("continue-cli").unwrap();
    let hook_input = json!({
        "session_id": "test-session",
        "transcript_path": "tests/fixtures/continue-cli-session-simple.json",
        "cwd": "/path",
        "model": "gpt-4",
        "hook_event_name": "PreToolUse",
        "tool_input": {
            "file_path": "/file.py"
        }
    })
    .to_string();

    let events = preset
        .parse(&hook_input, "t_test")
        .expect("Should succeed for PreToolUse");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.file_paths, vec![std::path::PathBuf::from("/file.py")]);
        }
        _ => panic!("Expected PreFileEdit for PreToolUse"),
    }
}

// ==============================================================================
// CodexPreset Error Cases
// ==============================================================================

#[test]
fn test_codex_preset_invalid_json() {
    let preset = resolve_preset("codex").unwrap();
    let result = preset.parse("{bad json", "t_test");

    assert!(result.is_err());
}

#[test]
fn test_codex_preset_missing_session_id() {
    let preset = resolve_preset("codex").unwrap();
    let hook_input = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "apply_patch",
        "transcript_path": "tests/fixtures/codex-session-simple.jsonl",
        "cwd": "/path"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    let msg = preset_error_message(
        result,
        "Expected PresetError for missing session_id/thread_id",
    );
    assert!(msg.contains("session_id") || msg.contains("thread_id"));
}

#[test]
fn test_codex_preset_invalid_transcript_path() {
    let preset = resolve_preset("codex").unwrap();
    let hook_input = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "patch-1",
        "session_id": "test-session-12345",
        "transcript_path": "/nonexistent/path/transcript.jsonl",
        "cwd": "/path"
    })
    .to_string();

    let events = preset
        .parse(&hook_input, "t_test")
        .expect("Should succeed with fallback to empty transcript");

    // parse() doesn't read the transcript, it just records the path
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(e.stream_source.is_some());
            assert_eq!(e.context.agent_id.model, "unknown");
            assert_eq!(e.context.agent_id.id, "test-session-12345");
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_continue_preset_with_tool_input_no_file_path() {
    let preset = resolve_preset("continue-cli").unwrap();
    let hook_input = json!({
        "session_id": "test",
        "transcript_path": "tests/fixtures/continue-cli-session-simple.json",
        "cwd": "/path",
        "model": "gpt-4",
        "tool_input": {}
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
