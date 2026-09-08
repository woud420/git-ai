use super::{
    ByteOffsetWatermark, GeminiAgent, GitAiError, ParsedHookEvent, fs, json, resolve_preset,
};
use git_ai::operations::streams::agent::Agent;

// ==============================================================================
// GeminiPreset Error Cases
// ==============================================================================

#[test]
fn test_gemini_preset_invalid_json() {
    let preset = resolve_preset("gemini").unwrap();
    let result = preset.parse("invalid{json", "t_test");

    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => {
            assert!(msg.contains("Invalid JSON"));
        }
        _ => panic!("Expected PresetError"),
    }
}

#[test]
fn test_gemini_preset_missing_session_id() {
    let preset = resolve_preset("gemini").unwrap();
    let hook_input = json!({
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl",
        "cwd": "/path"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => {
            assert!(msg.contains("session_id not found"));
        }
        _ => panic!("Expected PresetError"),
    }
}

#[test]
fn test_gemini_preset_missing_transcript_path() {
    let preset = resolve_preset("gemini").unwrap();
    let hook_input = json!({
        "session_id": "test-session",
        "cwd": "/path"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => {
            assert!(msg.contains("transcript_path not found"));
        }
        _ => panic!("Expected PresetError"),
    }
}

#[test]
fn test_gemini_preset_missing_cwd() {
    let preset = resolve_preset("gemini").unwrap();
    let hook_input = json!({
        "session_id": "test-session",
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => {
            assert!(msg.contains("cwd not found"));
        }
        _ => panic!("Expected PresetError"),
    }
}

#[test]
fn test_gemini_preset_beforetool_checkpoint() {
    let preset = resolve_preset("gemini").unwrap();
    let hook_input = json!({
        "session_id": "test-session",
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl",
        "cwd": "/path",
        "hook_event_name": "BeforeTool",
        "tool_input": {
            "file_path": "/file.js"
        }
    })
    .to_string();

    let events = preset
        .parse(&hook_input, "t_test")
        .expect("Should succeed for BeforeTool");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.file_paths, vec![std::path::PathBuf::from("/file.js")]);
        }
        _ => panic!("Expected PreFileEdit for BeforeTool"),
    }
}

#[test]
fn test_gemini_transcript_parsing_invalid_path() {
    let result = GeminiAgent::new().read_incremental(
        std::path::Path::new("/nonexistent/path.jsonl"),
        Box::new(ByteOffsetWatermark::new(0)),
        "test",
    );

    assert!(result.is_err());
    match result {
        Err(git_ai::operations::streams::StreamError::Fatal { .. }) => {}
        _ => panic!("Expected Fatal error for nonexistent path"),
    }
}

#[test]
fn test_gemini_transcript_parsing_empty_file() {
    let temp_file = std::env::temp_dir().join("gemini_empty.jsonl");
    fs::write(&temp_file, "").expect("Failed to write temp file");

    let result = GeminiAgent::new().read_incremental(
        &temp_file,
        Box::new(ByteOffsetWatermark::new(0)),
        "test",
    );

    assert!(result.is_ok());
    let batch = result.unwrap();
    assert!(batch.events.is_empty());

    fs::remove_file(temp_file).ok();
}

#[test]
fn test_gemini_transcript_parsing_invalid_json_line() {
    let temp_file = std::env::temp_dir().join("gemini_invalid_line.jsonl");
    fs::write(&temp_file, "this is not valid json\n").expect("Failed to write temp file");

    let result = GeminiAgent::new().read_incremental(
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
fn test_gemini_preset_with_tool_input_no_file_path() {
    let preset = resolve_preset("gemini").unwrap();
    let hook_input = json!({
        "session_id": "test",
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl",
        "cwd": "/path",
        "tool_input": {
            "other": "value"
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
fn test_gemini_transcript_with_unknown_message_types() {
    use std::io::Write;
    let temp_file = std::env::temp_dir().join("gemini_unknown_types.jsonl");
    let mut f = fs::File::create(&temp_file).unwrap();
    writeln!(f, r#"{{"type":"user","content":"test"}}"#).unwrap();
    writeln!(
        f,
        r#"{{"type":"unknown_type","content":"should still be included"}}"#
    )
    .unwrap();
    writeln!(
        f,
        r#"{{"type":"info","content":"should also be included"}}"#
    )
    .unwrap();
    writeln!(f, r#"{{"type":"gemini","content":"response"}}"#).unwrap();

    let batch = GeminiAgent::new()
        .read_incremental(&temp_file, Box::new(ByteOffsetWatermark::new(0)), "test")
        .expect("Should parse successfully");

    assert_eq!(batch.events.len(), 4);

    fs::remove_file(temp_file).ok();
}

#[test]
fn test_gemini_transcript_with_empty_tool_calls() {
    use std::io::Write;
    let temp_file = std::env::temp_dir().join("gemini_empty_tools.jsonl");
    let mut f = fs::File::create(&temp_file).unwrap();
    writeln!(f, r#"{{"type":"gemini","content":"test","toolCalls":[]}}"#).unwrap();

    let batch = GeminiAgent::new()
        .read_incremental(&temp_file, Box::new(ByteOffsetWatermark::new(0)), "test")
        .expect("Should parse successfully");

    assert_eq!(batch.events.len(), 1);

    fs::remove_file(temp_file).ok();
}

#[test]
fn test_gemini_transcript_tool_call_without_args() {
    use std::io::Write;
    let temp_file = std::env::temp_dir().join("gemini_tool_no_args.jsonl");
    let mut f = fs::File::create(&temp_file).unwrap();
    writeln!(
        f,
        r#"{{"type":"gemini","toolCalls":[{{"name":"read_file"}}]}}"#
    )
    .unwrap();

    let batch = GeminiAgent::new()
        .read_incremental(&temp_file, Box::new(ByteOffsetWatermark::new(0)), "test")
        .expect("Should parse successfully");

    let tool_messages: Vec<_> = batch
        .events
        .iter()
        .filter(|e| e["toolCalls"].as_array().is_some_and(|a| !a.is_empty()))
        .collect();
    assert_eq!(tool_messages.len(), 1);

    fs::remove_file(temp_file).ok();
}
