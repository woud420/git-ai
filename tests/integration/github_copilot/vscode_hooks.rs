use super::{
    ParsedHookEvent, ensure_clean_env, fs, json, parse_copilot,
    setup_vscode_model_lookup_workspace, vscode_post_tool_use_hook_input,
};

// NOTE: copilot_session_parsing_multiline_jsonl, copilot_session_jsonl_empty_snapshot_with_patch,
// copilot_session_jsonl_model_from_input_state_no_requests,
// copilot_session_jsonl_per_request_model_overrides_input_state, and
// copilot_session_jsonl_scalar_patch_applied were removed because the new CopilotAgent API
// does not support the kind:0/kind:1 JSONL snapshot+patch protocol.

// ============================================================================
// VS Code PreToolUse / PostToolUse tests
// ============================================================================

#[test]
fn test_copilot_preset_vscode_pretooluse_human_checkpoint() {
    let hook_input = json!({
        "hookEventName": "PreToolUse",
        "cwd": "/Users/test/project",
        "toolName": "copilot_replaceString",
        "transcript_path": "/Users/test/Library/Application Support/Code/User/workspaceStorage/ws-id/GitHub.copilot-chat/transcripts/session.jsonl",
        "toolInput": { "file_path": "src/main.ts" },
        "sessionId": "copilot-session-pre"
    })
    .to_string();

    let events = parse_copilot(&hook_input).expect("Expected human checkpoint");
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert!(
                e.file_paths
                    .iter()
                    .any(|p| p.to_string_lossy().contains("src/main.ts"))
            );
        }
        _ => panic!("Expected PreFileEdit for PreToolUse"),
    }
}

#[test]
fn test_copilot_preset_vscode_create_file_tool_is_supported() {
    let hook_input = json!({
        "hookEventName": "PreToolUse",
        "cwd": "/Users/test/project",
        "toolName": "create_file",
        "transcript_path": "/Users/test/Library/Application Support/Code/User/workspaceStorage/ws-id/GitHub.copilot-chat/transcripts/session.jsonl",
        "toolInput": { "filePath": "/Users/test/project/src/new-file.ts", "content": "export const x = 1;\n" },
        "sessionId": "copilot-session-create"
    }).to_string();

    let events = parse_copilot(&hook_input).expect("Expected human checkpoint");
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert!(
                e.file_paths
                    .iter()
                    .any(|p| p.to_string_lossy().contains("new-file.ts"))
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_copilot_preset_vscode_apply_patch_tool_is_supported() {
    let hook_input = json!({
        "hookEventName": "PreToolUse",
        "cwd": "/Users/test/project",
        "toolName": "apply_patch",
        "transcript_path": "/Users/test/Library/Application Support/Code/User/workspaceStorage/ws-id/GitHub.copilot-chat/transcripts/session.jsonl",
        "toolInput": "*** Begin Patch\n*** Update File: src/main.ts\n@@\n-old\n+new\n*** End Patch",
        "sessionId": "copilot-session-apply-patch"
    })
    .to_string();

    let events = parse_copilot(&hook_input).expect("Expected human checkpoint");
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert!(
                e.file_paths
                    .iter()
                    .any(|p| p.to_string_lossy().contains("src/main.ts"))
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_copilot_preset_vscode_editfiles_files_array_is_supported() {
    let hook_input = json!({
        "hookEventName": "PreToolUse",
        "cwd": "/Users/test/project",
        "toolName": "editFiles",
        "transcript_path": "/Users/test/Library/Application Support/Code/User/workspaceStorage/ws-id/GitHub.copilot-chat/transcripts/session.jsonl",
        "toolInput": { "files": ["src/main.ts", "/Users/test/project/src/other.ts"] },
        "sessionId": "copilot-session-editfiles"
    })
    .to_string();

    let events = parse_copilot(&hook_input).expect("Expected human checkpoint");
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.file_paths.len(), 2);
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_copilot_preset_vscode_posttooluse_ai_checkpoint() {
    let temp_dir = tempfile::tempdir().unwrap();
    let transcripts_dir = temp_dir
        .path()
        .join("workspaceStorage")
        .join("workspace-id")
        .join("GitHub.copilot-chat")
        .join("transcripts");
    fs::create_dir_all(&transcripts_dir).unwrap();
    let transcript_path = transcripts_dir.join("copilot-session-post.jsonl");
    fs::write(&transcript_path, r#"{"requests": []}"#).unwrap();
    let session_path = transcript_path.to_string_lossy().to_string();

    let hook_input = json!({
        "hookEventName": "PostToolUse",
        "cwd": "/Users/test/project",
        "toolName": "copilot_replaceString",
        "toolInput": { "file_path": "/Users/test/project/src/main.ts" },
        "sessionId": "copilot-session-post",
        "transcript_path": session_path
    })
    .to_string();

    let events = parse_copilot(&hook_input).expect("Expected AI checkpoint");
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert_eq!(e.context.agent_id.id, "copilot-session-post");
            assert!(
                e.file_paths
                    .iter()
                    .any(|p| p.to_string_lossy().contains("src/main.ts"))
            );
        }
        _ => panic!("Expected PostFileEdit for PostToolUse"),
    }
}

#[test]
fn test_copilot_preset_vscode_apply_patch_posttooluse_ai_checkpoint() {
    let temp_dir = tempfile::tempdir().unwrap();
    let transcripts_dir = temp_dir
        .path()
        .join("workspaceStorage")
        .join("workspace-id")
        .join("GitHub.copilot-chat")
        .join("transcripts");
    fs::create_dir_all(&transcripts_dir).unwrap();
    let transcript_path = transcripts_dir.join("copilot-session-apply-patch-post.jsonl");
    fs::write(&transcript_path, r#"{"requests": []}"#).unwrap();
    let session_path = transcript_path.to_string_lossy().to_string();

    let hook_input = json!({
        "hookEventName": "PostToolUse",
        "cwd": "/Users/test/project",
        "toolName": "apply_patch",
        "toolInput": "*** Begin Patch\n*** Update File: src/main.ts\n@@\n-old\n+new\n*** End Patch",
        "sessionId": "copilot-session-apply-patch-post",
        "transcript_path": session_path
    })
    .to_string();

    let events = parse_copilot(&hook_input).expect("Expected AI checkpoint");
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "github-copilot");
            assert_eq!(e.context.agent_id.id, "copilot-session-apply-patch-post");
            assert!(
                e.file_paths
                    .iter()
                    .any(|p| p.to_string_lossy().contains("src/main.ts"))
            );
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_copilot_preset_vscode_non_edit_tool_is_filtered() {
    let hook_input = json!({
        "hookEventName": "PreToolUse",
        "cwd": "/Users/test/project",
        "toolName": "copilot_findTextInFiles",
        "toolInput": { "query": "hello" },
        "sessionId": "copilot-session-search",
        "transcript_path": "/Users/test/Library/Application Support/Code/User/workspaceStorage/ws-id/GitHub.copilot-chat/transcripts/session.jsonl"
    })
    .to_string();

    let result = parse_copilot(&hook_input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("unsupported tool_name")
    );
}

#[test]
fn test_copilot_preset_vscode_claude_transcript_path_is_rejected() {
    let hook_input = json!({
        "hookEventName": "PostToolUse",
        "cwd": "/Users/test/project",
        "toolName": "copilot_replaceString",
        "toolInput": { "file_path": "/Users/test/project/src/main.ts" },
        "sessionId": "copilot-session-wrong",
        "transcript_path": "/Users/test/.claude/projects/session.jsonl"
    })
    .to_string();

    let result = parse_copilot(&hook_input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Claude transcript path")
    );
}

#[test]
#[serial_test::serial(copilot_env)]
fn test_copilot_preset_vscode_model_uses_auto_model_id_when_present() {
    ensure_clean_env();
    let (_temp_dir, transcript_path) =
        setup_vscode_model_lookup_workspace("copilot_chat_session_lookup_auto.jsonl");
    let events = parse_copilot(&vscode_post_tool_use_hook_input(&transcript_path))
        .expect("Expected AI checkpoint");
    match &events[0] {
        // Model is lazily resolved from transcript, so at parse time it's "unknown"
        ParsedHookEvent::PostFileEdit(e) => assert_eq!(e.context.agent_id.model, "unknown"),
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
#[serial_test::serial(copilot_env)]
fn test_copilot_preset_vscode_model_prefers_non_auto_model_id_from_chat_sessions() {
    ensure_clean_env();
    let (_temp_dir, transcript_path) =
        setup_vscode_model_lookup_workspace("copilot_chat_session_lookup_non_auto.jsonl");
    let events = parse_copilot(&vscode_post_tool_use_hook_input(&transcript_path))
        .expect("Expected AI checkpoint");
    match &events[0] {
        // Model is lazily resolved from transcript, so at parse time it's "unknown"
        ParsedHookEvent::PostFileEdit(e) => assert_eq!(e.context.agent_id.model, "unknown"),
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
#[serial_test::serial(copilot_env)]
fn test_copilot_preset_vscode_model_falls_back_to_selected_model_id() {
    ensure_clean_env();
    let (_temp_dir, transcript_path) =
        setup_vscode_model_lookup_workspace("copilot_chat_session_lookup_selected_model.jsonl");
    let events = parse_copilot(&vscode_post_tool_use_hook_input(&transcript_path))
        .expect("Expected AI checkpoint");
    match &events[0] {
        // Model is lazily resolved from transcript, so at parse time it's "unknown"
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.model, "unknown")
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
#[serial_test::serial(copilot_env)]
fn test_copilot_preset_vscode_model_lookup_supports_json_chat_session_file() {
    ensure_clean_env();
    let (_temp_dir, transcript_path) =
        setup_vscode_model_lookup_workspace("copilot_chat_session_lookup_json_file.json");
    let events = parse_copilot(&vscode_post_tool_use_hook_input(&transcript_path))
        .expect("Expected AI checkpoint");
    match &events[0] {
        // Model is lazily resolved from transcript, so at parse time it's "unknown"
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.model, "unknown")
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
#[serial_test::serial(copilot_env)]
fn test_copilot_preset_vscode_does_not_use_details_as_model_fallback() {
    ensure_clean_env();
    let (_temp_dir, transcript_path) =
        setup_vscode_model_lookup_workspace("copilot_chat_session_lookup_details_only.jsonl");
    let events = parse_copilot(&vscode_post_tool_use_hook_input(&transcript_path))
        .expect("Expected AI checkpoint");
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => assert_eq!(e.context.agent_id.model, "unknown"),
        _ => panic!("Expected PostFileEdit"),
    }
}
