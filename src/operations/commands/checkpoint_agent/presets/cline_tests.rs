use super::*;
use crate::operations::commands::checkpoint_agent::presets::*;
use serde_json::json;

fn make_cline_input(event: &str, tool: &str, parameters: Value) -> String {
    json!({
        "hookName": event,
        "clineVersion": "3.0.0",
        "taskId": "cline-task-123",
        "workspaceRoots": ["/home/user/project"],
        "userId": "user-1",
        "model": { "provider": "anthropic", "slug": "claude-sonnet-4-6" },
        "preToolUse": {
            "toolName": tool,
            "parameters": parameters
        }
    })
    .to_string()
}

fn make_post_cline_input(tool: &str, parameters: Value, result: &str) -> String {
    json!({
        "hookName": "PostToolUse",
        "clineVersion": "3.0.0",
        "taskId": "cline-task-123",
        "workspaceRoots": ["/home/user/project"],
        "model": { "provider": "anthropic", "slug": "claude-sonnet-4-6" },
        "postToolUse": {
            "toolName": tool,
            "parameters": parameters,
            "result": result,
            "success": true,
            "executionTimeMs": 123
        }
    })
    .to_string()
}

#[test]
fn test_cline_pre_file_edit_editor() {
    let input = make_cline_input(
        "PreToolUse",
        "editor",
        json!({ "path": "src/main.rs", "old_text": "old", "new_text": "new" }),
    );
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "cline");
            assert_eq!(e.context.external_session_id, "cline-task-123");
            assert_eq!(e.context.cwd, PathBuf::from("/home/user/project"));
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/main.rs")]
            );
            assert_eq!(e.context.agent_id.model, "claude-sonnet-4-6");
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_cline_post_file_edit_apply_patch() {
    let input = make_post_cline_input(
        "apply_patch",
        json!({
            "input": "*** Update File: src/main.rs\n@@ old\n+new\n"
        }),
        "done",
    );
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.context.agent_id.tool, "cline");
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/main.rs")]
            );
            assert!(e.stream_source.is_none());
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_cline_post_file_edit_write_to_file() {
    let input = make_post_cline_input(
        "write_to_file",
        json!({
            "path": "src/lib.rs",
            "content": "fn main() {}"
        }),
        "done",
    );
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(
                e.file_paths,
                vec![PathBuf::from("/home/user/project/src/lib.rs")]
            );
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_cline_pre_bash_execute_command() {
    let input = make_cline_input(
        "PreToolUse",
        "execute_command",
        json!({ "command": "cargo test" }),
    );
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreBashCall(e) => {
            assert_eq!(e.context.agent_id.tool, "cline");
            assert_eq!(e.command.as_deref(), Some("cargo test"));
        }
        _ => panic!("Expected PreBashCall"),
    }
}

#[test]
fn test_cline_pre_bash_run_commands_array() {
    let input = make_cline_input(
        "PreToolUse",
        "run_commands",
        json!({ "commands": ["echo a", { "command": "echo b", "cwd": "/tmp" }] }),
    );
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ParsedHookEvent::PreBashCall(e) => {
            assert_eq!(e.command.as_deref(), Some("echo a; echo b"));
        }
        _ => panic!("Expected PreBashCall"),
    }
}

#[test]
fn test_cline_skips_unsupported_tool() {
    let input = make_cline_input(
        "PreToolUse",
        "read_files",
        json!({ "files": ["src/main.rs"] }),
    );
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    assert!(events.is_empty());
}

#[test]
fn test_cline_skips_non_tool_hooks() {
    let input = json!({
        "hookName": "TaskComplete",
        "clineVersion": "3.0.0",
        "taskId": "cline-task-123",
        "workspaceRoots": ["/home/user/project"],
    })
    .to_string();
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    assert!(events.is_empty());
}

#[test]
fn test_cline_model_fallback_to_provider() {
    let input = json!({
        "hookName": "PreToolUse",
        "clineVersion": "3.0.0",
        "taskId": "cline-task-123",
        "workspaceRoots": ["/home/user/project"],
        "model": { "provider": "openai", "slug": "" },
        "preToolUse": { "toolName": "editor", "parameters": { "path": "x.rs" } }
    })
    .to_string();
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.context.agent_id.model, "openai");
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_cline_model_allows_missing_fields() {
    for (model, expected) in [
        (json!({ "provider": "openai" }), "openai"),
        (json!({ "slug": "gpt-5" }), "gpt-5"),
    ] {
        let input = json!({
            "hookName": "PreToolUse",
            "clineVersion": "3.0.0",
            "taskId": "cline-task-123",
            "workspaceRoots": ["/home/user/project"],
            "model": model,
            "preToolUse": {
                "toolName": "editor",
                "parameters": { "path": "x.rs" }
            }
        })
        .to_string();

        let events = ClinePreset.parse(&input, "t_test").unwrap();
        match &events[0] {
            ParsedHookEvent::PreFileEdit(e) => {
                assert_eq!(e.context.agent_id.model, expected);
            }
            _ => panic!("Expected PreFileEdit"),
        }
    }
}

#[test]
fn test_cline_parameters_stringified_array() {
    let input = json!({
        "hookName": "PreToolUse",
        "clineVersion": "3.0.0",
        "taskId": "cline-task-123",
        "workspaceRoots": ["/home/user/project"],
        "preToolUse": {
            "toolName": "editor",
            "parameters": {
                "files": "[\"src/a.rs\", \"src/b.rs\"]"
            }
        }
    })
    .to_string();
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.file_paths.len(), 2);
            assert!(
                e.file_paths
                    .contains(&PathBuf::from("/home/user/project/src/a.rs"))
            );
            assert!(
                e.file_paths
                    .contains(&PathBuf::from("/home/user/project/src/b.rs"))
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_cline_extracts_paths_array() {
    let input = json!({
        "hookName": "PreToolUse",
        "clineVersion": "3.0.0",
        "taskId": "cline-task-123",
        "workspaceRoots": ["/home/user/project"],
        "preToolUse": {
            "toolName": "editor",
            "parameters": {
                "paths": ["src/a.rs", { "path": "src/b.rs" }]
            }
        }
    })
    .to_string();
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    match &events[0] {
        ParsedHookEvent::PreFileEdit(e) => {
            assert_eq!(e.file_paths.len(), 2);
            assert!(
                e.file_paths
                    .contains(&PathBuf::from("/home/user/project/src/a.rs"))
            );
            assert!(
                e.file_paths
                    .contains(&PathBuf::from("/home/user/project/src/b.rs"))
            );
        }
        _ => panic!("Expected PreFileEdit"),
    }
}

#[test]
fn test_cline_content_not_treated_as_patch() {
    let input = make_post_cline_input(
        "write_to_file",
        json!({
            "path": "src/main.rs",
            "content": "*** Update File: src/other.rs\n@@ old\n+new\n"
        }),
        "done",
    );
    let events = ClinePreset.parse(&input, "t_test").unwrap();
    match &events[0] {
        ParsedHookEvent::PostFileEdit(e) => {
            assert_eq!(e.file_paths.len(), 1);
            assert_eq!(
                e.file_paths[0],
                PathBuf::from("/home/user/project/src/main.rs")
            );
        }
        _ => panic!("Expected PostFileEdit"),
    }
}

#[test]
fn test_cline_tool_use_id_is_deterministic() {
    let input = make_cline_input("PreToolUse", "editor", json!({ "path": "src/main.rs" }));
    let id1 = match &ClinePreset.parse(&input, "t1").unwrap()[0] {
        ParsedHookEvent::PreFileEdit(e) => e.tool_use_id.clone().unwrap(),
        _ => panic!(),
    };
    let id2 = match &ClinePreset.parse(&input, "t2").unwrap()[0] {
        ParsedHookEvent::PreFileEdit(e) => e.tool_use_id.clone().unwrap(),
        _ => panic!(),
    };
    assert_eq!(id1, id2);
}
