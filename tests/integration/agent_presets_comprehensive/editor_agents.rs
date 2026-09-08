use super::{GitAiError, ParsedHookEvent, json, resolve_preset};

// ==============================================================================
// CursorPreset Error Cases
// ==============================================================================

#[test]
fn test_cursor_preset_invalid_json() {
    let preset = resolve_preset("cursor").unwrap();
    let result = preset.parse("invalid", "t_test");

    assert!(result.is_err());
}

#[test]
fn test_cursor_preset_missing_conversation_id() {
    let preset = resolve_preset("cursor").unwrap();
    let hook_input = json!({
        "type": "composer_turn_complete",
        "cwd": "/path"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => {
            assert!(msg.contains("conversation_id not found"));
        }
        _ => panic!("Expected PresetError"),
    }
}

#[test]
fn test_cursor_preset_missing_workspace_roots() {
    let preset = resolve_preset("cursor").unwrap();
    let hook_input = json!({
        "type": "composer_turn_complete",
        "conversation_id": "test-conv",
        "hook_event_name": "afterFileEdit"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => {
            assert!(msg.contains("workspace_roots not found"));
        }
        _ => panic!("Expected PresetError for missing workspace_roots"),
    }
}

// ==============================================================================
// GithubCopilotPreset Error Cases
// ==============================================================================

#[test]
fn test_github_copilot_preset_invalid_json() {
    let preset = resolve_preset("github-copilot").unwrap();
    let result = preset.parse("not json", "t_test");

    assert!(result.is_err());
}

#[test]
fn test_github_copilot_preset_invalid_hook_event_name() {
    let preset = resolve_preset("github-copilot").unwrap();
    let hook_input = json!({
        "hook_event_name": "invalid_event_name",
        "sessionId": "test-session",
        "transcriptPath": "tests/fixtures/copilot_session_simple.jsonl"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => {
            assert!(msg.contains("Invalid hook_event_name"));
            assert!(msg.contains("before_edit") || msg.contains("after_edit"));
        }
        _ => panic!("Expected PresetError for invalid hook_event_name"),
    }
}

// ==============================================================================
// DroidPreset Error Cases
// ==============================================================================

#[test]
fn test_droid_preset_invalid_json() {
    let preset = resolve_preset("droid").unwrap();
    let result = preset.parse("{invalid", "t_test");

    assert!(result.is_err());
}

#[test]
fn test_droid_preset_generates_fallback_session_id() {
    let preset = resolve_preset("droid").unwrap();
    let hook_input = json!({
        "transcript_path": "tests/fixtures/droid-session.jsonl",
        "cwd": "/path",
        "hookEventName": "PostToolUse",
        "toolName": "Edit"
    })
    .to_string();

    let events = preset
        .parse(&hook_input, "t_test")
        .expect("Should succeed with generated session_id");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(e.context.agent_id.id.starts_with("droid-"));
            assert_eq!(e.context.agent_id.tool, "droid");
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

// ==============================================================================
// AiTabPreset Error Cases
// ==============================================================================

#[test]
fn test_aitab_preset_invalid_json() {
    let preset = resolve_preset("ai_tab").unwrap();
    let result = preset.parse("bad json", "t_test");

    assert!(result.is_err());
}

#[test]
fn test_aitab_preset_invalid_hook_event_name() {
    let preset = resolve_preset("ai_tab").unwrap();
    let hook_input = json!({
        "hook_event_name": "invalid_event",
        "tool": "test_tool",
        "model": "test_model"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => {
            assert!(msg.contains("Unsupported hook_event_name"));
            assert!(msg.contains("expected 'before_edit' or 'after_edit'"));
        }
        _ => panic!("Expected PresetError"),
    }
}

#[test]
fn test_aitab_preset_empty_tool() {
    let preset = resolve_preset("ai_tab").unwrap();
    let hook_input = json!({
        "hook_event_name": "after_edit",
        "tool": "  ",
        "model": "test_model"
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => {
            assert!(msg.contains("tool must be a non-empty string"));
        }
        _ => panic!("Expected PresetError"),
    }
}

#[test]
fn test_aitab_preset_empty_model() {
    let preset = resolve_preset("ai_tab").unwrap();
    let hook_input = json!({
        "hook_event_name": "after_edit",
        "tool": "test_tool",
        "model": "  "
    })
    .to_string();

    let result = preset.parse(&hook_input, "t_test");

    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => {
            assert!(msg.contains("model must be a non-empty string"));
        }
        _ => panic!("Expected PresetError"),
    }
}

#[test]
fn test_aitab_preset_before_edit_checkpoint() {
    let preset = resolve_preset("ai_tab").unwrap();
    let hook_input = json!({
        "hook_event_name": "before_edit",
        "tool": "test_tool",
        "model": "gpt-4",
        "repo_working_dir": "/project",
        "will_edit_filepaths": ["/file1.rs", "/file2.rs"]
    })
    .to_string();

    let events = preset
        .parse(&hook_input, "t_test")
        .expect("Should succeed for before_edit");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "test_tool");
            assert_eq!(e.context.agent_id.model, "gpt-4");
            assert_eq!(
                e.file_paths,
                vec![
                    std::path::PathBuf::from("/file1.rs"),
                    std::path::PathBuf::from("/file2.rs"),
                ]
            );
        }
        _ => panic!("Expected PreFileEdit for before_edit"),
    }
}

#[test]
fn test_aitab_preset_after_edit_checkpoint() {
    let preset = resolve_preset("ai_tab").unwrap();
    let hook_input = json!({
        "hook_event_name": "after_edit",
        "tool": "test_tool",
        "model": "gpt-4",
        "repo_working_dir": "/project",
        "edited_filepaths": ["/file1.rs"]
    })
    .to_string();

    let events = preset
        .parse(&hook_input, "t_test")
        .expect("Should succeed for after_edit");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(e.stream_source.is_none());
            assert_eq!(e.file_paths, vec![std::path::PathBuf::from("/file1.rs")]);
        }
        _ => panic!("Expected PostFileEdit for after_edit"),
    }
}

#[test]
fn test_aitab_preset_with_dirty_files() {
    let preset = resolve_preset("ai_tab").unwrap();
    let mut dirty_files = std::collections::HashMap::new();
    dirty_files.insert("/file1.rs".to_string(), "content1".to_string());
    dirty_files.insert("/file2.rs".to_string(), "content2".to_string());

    let hook_input = json!({
        "hook_event_name": "after_edit",
        "tool": "test_tool",
        "model": "gpt-4",
        "dirty_files": dirty_files
    })
    .to_string();

    let events = preset
        .parse(&hook_input, "t_test")
        .expect("Should succeed with dirty_files");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(e.dirty_files.is_some());
            let dirty = e.dirty_files.as_ref().unwrap();
            assert_eq!(dirty.len(), 2);
            assert_eq!(
                dirty.get(&std::path::PathBuf::from("/file1.rs")),
                Some(&"content1".to_string())
            );
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_aitab_preset_empty_repo_working_dir_filtered() {
    let preset = resolve_preset("ai_tab").unwrap();
    let hook_input = json!({
        "hook_event_name": "after_edit",
        "tool": "test_tool",
        "model": "gpt-4",
        "repo_working_dir": "   "
    })
    .to_string();

    let events = preset.parse(&hook_input, "t_test").expect("Should succeed");

    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            // Empty/whitespace-only repo_working_dir should fall back to "."
            assert_eq!(e.context.cwd, std::path::PathBuf::from("."));
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

// ==============================================================================
// Integration Tests - Cross-Preset Behavior
// ==============================================================================

#[test]
fn test_all_presets_handle_invalid_json_consistently() {
    let preset_names = vec![
        "claude",
        "gemini",
        "continue-cli",
        "codex",
        "cursor",
        "github-copilot",
        "amp",
        "droid",
        "ai_tab",
    ];

    for name in preset_names {
        let preset = resolve_preset(name).unwrap();
        let result = preset.parse("{invalid json}", "t_test");
        assert!(
            result.is_err(),
            "Preset '{}' should fail with invalid JSON",
            name,
        );
    }
}
