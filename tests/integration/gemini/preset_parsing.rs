use super::{
    ByteOffsetWatermark, GeminiAgent, ParsedHookEvent, fixture_path, json, parse_gemini,
    read_jsonl_fixture,
};
use git_ai::operations::streams::agent::Agent;

#[test]
fn test_gemini_raw_event_fidelity() {
    let fixture = fixture_path("gemini-session-simple.jsonl");
    let agent = GeminiAgent::new();
    let watermark = Box::new(ByteOffsetWatermark::new(0));
    let result = agent
        .read_incremental(fixture.as_path(), watermark, "test")
        .unwrap();

    let expected = read_jsonl_fixture(&fixture).unwrap();

    assert_eq!(result.events.len(), expected.len());
    assert_eq!(result.events, expected);
}

#[test]
fn test_gemini_preset_extracts_edited_filepath() {
    let hook_input = json!({
        "cwd": "/Users/svarlamov/projects/testing-git",
        "hook_event_name": "AfterTool",
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "tool_input": {
            "file_path": "/Users/svarlamov/projects/testing-git/index.ts"
        },
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl"
    })
    .to_string();

    let events = parse_gemini(&hook_input).expect("Failed to run GeminiPreset");
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(!e.file_paths.is_empty());
            assert!(
                e.file_paths
                    .iter()
                    .any(|p| p.to_string_lossy().contains("index.ts")),
                "Should contain edited filepath"
            );
        }
        _ => panic!("Expected PostFileEdit for AfterTool"),
    }
}

#[test]
fn test_gemini_preset_no_filepath_when_tool_input_missing() {
    let hook_input = json!({
        "cwd": "/Users/svarlamov/projects/testing-git",
        "hook_event_name": "AfterTool",
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl"
    })
    .to_string();

    let events = parse_gemini(&hook_input).expect("Failed to run GeminiPreset");
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(
                e.file_paths.is_empty(),
                "edited_filepaths should be empty when tool_input is missing"
            );
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_gemini_preset_human_checkpoint() {
    let hook_input = json!({
        "cwd": "/Users/svarlamov/projects/testing-git",
        "hook_event_name": "BeforeTool",
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "tool_input": {
            "file_path": "/Users/svarlamov/projects/testing-git/index.ts"
        },
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl"
    })
    .to_string();

    let events = parse_gemini(&hook_input).expect("Failed to run GeminiPreset");
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert!(
                e.file_paths
                    .iter()
                    .any(|p| p.to_string_lossy().contains("index.ts")),
                "Should have will_edit_filepaths"
            );
        }
        _ => panic!("Expected PreFileEdit for BeforeTool"),
    }
}

#[test]
fn test_gemini_preset_ai_checkpoint() {
    let hook_input = json!({
        "cwd": "/Users/svarlamov/projects/testing-git",
        "hook_event_name": "AfterTool",
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "tool_input": {
            "file_path": "/Users/svarlamov/projects/testing-git/index.ts"
        },
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl"
    })
    .to_string();

    let events = parse_gemini(&hook_input).expect("Failed to run GeminiPreset");
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(e.stream_source.is_some(), "Should have transcript");
            assert!(!e.file_paths.is_empty(), "Should have edited_filepaths");
        }
        _ => panic!("Expected PostFileEdit for AfterTool"),
    }
}

#[test]
fn test_gemini_preset_extracts_model() {
    let hook_input = json!({
        "cwd": "/Users/svarlamov/projects/testing-git",
        "hook_event_name": "AfterTool",
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl"
    })
    .to_string();

    let events = parse_gemini(&hook_input).expect("Failed to run GeminiPreset");
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.model, "gemini-2.5-flash");
            assert_eq!(e.context.agent_id.tool, "gemini");
            assert_eq!(
                e.context.agent_id.id,
                "18f475c0-690f-4bc9-b84e-88a0a1e9518f"
            );
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_gemini_preset_stores_transcript_path_in_metadata() {
    let hook_input = json!({
        "cwd": "/Users/svarlamov/projects/testing-git",
        "hook_event_name": "AfterTool",
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl"
    })
    .to_string();

    let events = parse_gemini(&hook_input).expect("Failed to run GeminiPreset");
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(
                e.context.metadata.get("transcript_path"),
                Some(&"tests/fixtures/gemini-session-simple.jsonl".to_string())
            );
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_gemini_preset_handles_missing_transcript_path() {
    let hook_input = json!({
        "cwd": "/Users/svarlamov/projects/testing-git",
        "hook_event_name": "AfterTool",
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f"
    })
    .to_string();

    let result = parse_gemini(&hook_input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("transcript_path not found")
    );
}

#[test]
fn test_gemini_preset_handles_invalid_json() {
    let result = parse_gemini("{ invalid json }");
    assert!(result.is_err());
}

#[test]
fn test_gemini_preset_handles_missing_session_id() {
    let hook_input = json!({
        "cwd": "/Users/svarlamov/projects/testing-git",
        "hook_event_name": "AfterTool",
        "transcript_path": "tests/fixtures/gemini-session-simple.jsonl"
    })
    .to_string();

    let result = parse_gemini(&hook_input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("session_id not found")
    );
}

#[test]
fn test_gemini_preset_handles_missing_file() {
    let hook_input = json!({
        "cwd": "/Users/svarlamov/projects/testing-git",
        "hook_event_name": "AfterTool",
        "session_id": "18f475c0-690f-4bc9-b84e-88a0a1e9518f",
        "transcript_path": "tests/fixtures/nonexistent.jsonl"
    })
    .to_string();

    let result = parse_gemini(&hook_input);
    assert!(result.is_ok());
    let events = result.unwrap();
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.model, "unknown");
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

crate::reuse_tests_in_worktree!(
    test_gemini_raw_event_fidelity,
    test_gemini_preset_extracts_edited_filepath,
    test_gemini_preset_no_filepath_when_tool_input_missing,
    test_gemini_preset_human_checkpoint,
    test_gemini_preset_ai_checkpoint,
    test_gemini_preset_extracts_model,
    test_gemini_preset_stores_transcript_path_in_metadata,
    test_gemini_preset_handles_missing_transcript_path,
    test_gemini_preset_handles_invalid_json,
    test_gemini_preset_handles_missing_session_id,
    test_gemini_preset_handles_missing_file,
);
