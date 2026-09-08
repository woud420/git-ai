use super::*;
use crate::operations::commands::checkpoint_agent::presets::*;
use serde_json::json;

fn make_cursor_hook_input(event: &str, tool: &str) -> String {
    json!({
        "conversation_id": "conv-123",
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": event,
        "tool_name": tool,
        "model": "claude-3-5-sonnet",
        "transcript_path": "/home/user/.cursor/transcripts/conv-123.jsonl",
        "tool_input": {"file_path": "src/main.rs"}
    })
    .to_string()
}

#[test]
fn test_cursor_pre_file_edit() {
    let input = make_cursor_hook_input("preToolUse", "Write");
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "cursor");
            assert_eq!(e.context.external_session_id, "conv-123");
            assert_eq!(e.context.trace_id, "t_test123456789a");
            assert_eq!(e.context.agent_id.model, "claude-3-5-sonnet");
            assert_eq!(e.context.cwd, PathBuf::from("/home/user/project"));
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/main.rs")]
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_cursor_post_file_edit() {
    let input = make_cursor_hook_input("postToolUse", "Write");
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "cursor");
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/main.rs")]
            );
            assert!(e.stream_source.is_some());
            if let Some(ts) = &e.stream_source {
                assert_eq!(ts.format, StreamFormat::CursorJsonl);
                assert_eq!(ts.session_id, generate_session_id("conv-123", "cursor"));
                assert_eq!(ts.external_session_id, "conv-123");
            }
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_cursor_skips_non_edit_tools() {
    let input = json!({
        "conversation_id": "conv-123",
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": "preToolUse",
        "tool_name": "Read",
        "tool_input": {"file_path": "src/main.rs"}
    })
    .to_string();
    let result = CursorPreset.parse(&input, "t_test123456789a");
    assert!(result.is_err());
}

#[test]
fn test_cursor_skips_legacy_events() {
    let input = json!({
        "conversation_id": "conv-123",
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": "beforeSubmitPrompt",
    })
    .to_string();
    let result = CursorPreset.parse(&input, "t_test123456789a");
    assert!(result.is_err());
}

#[test]
fn test_cursor_requires_conversation_id() {
    let input = json!({
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": "preToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": "src/main.rs"}
    })
    .to_string();
    let result = CursorPreset.parse(&input, "t_test123456789a");
    assert!(result.is_err());
}

#[test]
fn test_cursor_absolute_file_path() {
    let input = json!({
        "conversation_id": "conv-123",
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": "preToolUse",
        "tool_name": "StrReplace",
        "tool_input": {"file_path": "/home/user/project/src/lib.rs"}
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/lib.rs")]
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_cursor_file_edit_accepts_path_field() {
    let input = json!({
        "conversation_id": "conv-123",
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": "preToolUse",
        "tool_name": "StrReplace",
        "tool_input": {"path": "/home/user/project/src/lib.rs"}
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/lib.rs")]
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_cursor_file_edit_prefers_file_path_over_path() {
    let input = json!({
        "conversation_id": "conv-123",
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": "preToolUse",
        "tool_name": "StrReplace",
        "tool_input": {
            "file_path": "src/from_file_path.rs",
            "path": "src/from_path.rs"
        }
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/from_file_path.rs")]
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_cursor_no_transcript_path() {
    let input = json!({
        "conversation_id": "conv-123",
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": "postToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": "src/main.rs"}
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert!(e.stream_source.is_none());
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_cursor_multiple_workspace_roots() {
    let input = json!({
        "conversation_id": "conv-123",
        "workspace_roots": ["/home/user/project-a", "/home/user/project-b"],
        "hook_event_name": "preToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": "/home/user/project-b/src/main.rs"}
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            // Should pick project-b as cwd since file is there
            assert_eq!(e.context.cwd, PathBuf::from("/home/user/project-b"));
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_cursor_delete_tool() {
    let input = make_cursor_hook_input("postToolUse", "Delete");
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ParsedHookEvent::PostFileEdit(_)));
}

#[test]
fn test_cursor_pre_shell_tool() {
    let input = json!({
        "conversation_id": "conv-shell",
        "session_id": "conv-shell",
        "workspace_roots": ["/Users/aidan/Desktop/test-repo"],
        "hook_event_name": "preToolUse",
        "tool_name": "Shell",
        "tool_use_id": "tu-shell-1",
        "model": "composer-2",
        "cursor_version": "3.1.17",
        "tool_input": {
            "command": "date > current_time.txt",
            "cwd": "",
            "timeout": 30000
        }
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreBashCall(e) => {
            assert_eq!(e.context.agent_id.tool, "cursor");
            assert_eq!(e.context.external_session_id, "conv-shell");
            assert_eq!(e.context.agent_id.model, "composer-2");
            assert_eq!(
                e.context.cwd,
                PathBuf::from("/Users/aidan/Desktop/test-repo")
            );
            assert_eq!(e.tool_use_id, "tu-shell-1");
            assert_eq!(e.command.as_deref(), Some("date > current_time.txt"));
        }
        _ => panic!("Expected PreBashCall, got {:?}", events[0]),
    }
}

#[test]
fn test_cursor_post_shell_tool() {
    let input = json!({
        "conversation_id": "conv-shell",
        "session_id": "conv-shell",
        "workspace_roots": ["/Users/aidan/Desktop/test-repo"],
        "hook_event_name": "postToolUse",
        "tool_name": "Shell",
        "tool_use_id": "tu-shell-2",
        "model": "composer-2",
        "tool_input": {
            "command": "date > current_time.txt",
            "cwd": ""
        }
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostBashCall(e) => {
            assert_eq!(e.context.agent_id.tool, "cursor");
            assert_eq!(e.tool_use_id, "tu-shell-2");
            assert_eq!(e.command.as_deref(), Some("date > current_time.txt"));
        }
        _ => panic!("Expected PostBashCall, got {:?}", events[0]),
    }
}

#[test]
fn test_cursor_shell_falls_back_to_default_tool_use_id() {
    let input = json!({
        "conversation_id": "conv-shell",
        "workspace_roots": ["/Users/aidan/Desktop/test-repo"],
        "hook_event_name": "preToolUse",
        "tool_name": "Shell",
        "tool_input": {"command": "ls"}
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    match &events[0] {
        ParsedHookEvent::PreBashCall(e) => {
            assert_eq!(e.tool_use_id, "bash");
        }
        _ => panic!("Expected PreBashCall"),
    }
}

#[test]
fn test_cursor_apply_patch_pre_file_edit() {
    let input = json!({
        "conversation_id": "conv-patch",
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": "preToolUse",
        "tool_name": "ApplyPatch",
        "model": "claude-3-5-sonnet",
        "tool_input": {
            "patch": "*** Begin Patch\n*** Update File: src/example.rs\n@@\n-old\n+new\n*** End Patch\n"
        }
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "cursor");
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/example.rs")]
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_cursor_apply_patch_post_file_edit() {
    let input = json!({
        "conversation_id": "conv-patch",
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": "postToolUse",
        "tool_name": "ApplyPatch",
        "model": "claude-3-5-sonnet",
        "transcript_path": "/home/user/.cursor/transcripts/conv-patch.jsonl",
        "tool_input": {
            "patch": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old line\n+new line\n*** Add File: src/new.rs\n@@\n+content\n*** End Patch\n"
        }
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "cursor");
            assert_eq!(e.file_paths.len(), 2);
            assert_eq!(
                e.file_paths[0],
                PathBuf::from("/home/user/project/src/main.rs")
            );
            assert_eq!(
                e.file_paths[1],
                PathBuf::from("/home/user/project/src/new.rs")
            );
            assert!(e.stream_source.is_some());
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_cursor_apply_patch_with_absolute_path_in_patch() {
    let input = json!({
        "conversation_id": "conv-patch",
        "workspace_roots": ["/home/user/project"],
        "hook_event_name": "preToolUse",
        "tool_name": "ApplyPatch",
        "tool_input": {
            "patch": "*** Update File: /home/user/project/src/lib.rs\nsome diff"
        }
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/lib.rs")]
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[cfg(windows)]
#[test]
fn test_cursor_apply_patch_normalizes_windows_path() {
    // Cursor can embed Unix-style `/c:/...` paths in patch text on Windows;
    // patch-extracted paths must be normalized just like JSON-field paths.
    let input = json!({
        "conversation_id": "conv-patch",
        "workspace_roots": ["C:\\Users\\project"],
        "hook_event_name": "preToolUse",
        "tool_name": "ApplyPatch",
        "tool_input": {
            "patch": "*** Update File: /c:/Users/project/src/main.rs\nsome diff"
        }
    })
    .to_string();
    let events = CursorPreset.parse(&input, "t_test123456789a").unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("C:\\Users\\project\\src\\main.rs")]
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_matching_workspace_root() {
    let roots = vec![
        "/home/user/project-a".to_string(),
        "/home/user/project-b".to_string(),
    ];
    assert_eq!(
        matching_workspace_root("/home/user/project-b/src/main.rs", &roots),
        Some("/home/user/project-b".to_string())
    );
    assert_eq!(matching_workspace_root("/other/path/file.rs", &roots), None);
}
