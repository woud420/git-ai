use super::super::parse;
use super::super::{
    ParsedHookEvent, PostBashCall, PostFileEdit, PreBashCall, PreFileEdit, PresetContext,
    StreamFormat, StreamSource,
};
use crate::error::GitAiError;
use crate::model::authorship_log_serialization::generate_session_id;
use crate::model::working_log::AgentId;
use crate::operations::commands::checkpoint_agent::bash_tool::ToolClass;
use std::collections::HashMap;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Legacy extension path (before_edit / after_edit)
// ---------------------------------------------------------------------------

pub(super) fn parse_legacy_extension_hooks(
    data: &serde_json::Value,
    hook_event_name: &str,
    trace_id: &str,
) -> Result<Vec<ParsedHookEvent>, GitAiError> {
    let cwd = parse::optional_str_multi(data, &["workspace_folder", "workspaceFolder"])
        .ok_or_else(|| {
            GitAiError::PresetError(
                "workspace_folder or workspaceFolder not found in hook_input for GitHub Copilot preset".to_string(),
            )
        })?;

    let dirty_files = super::dirty_files_from_hook_data(data, cwd);

    let session_id = super::extract_session_id(data);

    if hook_event_name == "before_edit" {
        let will_edit_filepaths = data
            .get("will_edit_filepaths")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| parse::resolve_absolute(s, cwd))
                    .collect::<Vec<PathBuf>>()
            })
            .ok_or_else(|| {
                GitAiError::PresetError(
                    "will_edit_filepaths is required for before_edit hook_event_name".to_string(),
                )
            })?;

        if will_edit_filepaths.is_empty() {
            return Err(GitAiError::PresetError(
                "will_edit_filepaths cannot be empty for before_edit hook_event_name".to_string(),
            ));
        }

        let context = PresetContext {
            agent_id: AgentId {
                tool: "github-copilot".to_string(),
                id: session_id.clone(),
                model: "unknown".to_string(),
            },
            external_session_id: session_id,
            trace_id: trace_id.to_string(),
            cwd: PathBuf::from(cwd),
            metadata: HashMap::new(),
        };

        return Ok(vec![ParsedHookEvent::PreFileEdit(PreFileEdit {
            context,
            file_paths: will_edit_filepaths,
            dirty_files,
            tool_use_id: None,
        })]);
    }

    // after_edit path
    let chat_session_path =
        parse::optional_str_multi(data, &["chat_session_path", "chatSessionPath"]).ok_or_else(
            || {
                GitAiError::PresetError(
                    "chat_session_path or chatSessionPath not found in hook_input for after_edit"
                        .to_string(),
                )
            },
        )?;

    let edited_filepaths = data
        .get("edited_filepaths")
        .and_then(|val| val.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| parse::resolve_absolute(s, cwd))
                .collect::<Vec<PathBuf>>()
        })
        .unwrap_or_default();

    let mut metadata = HashMap::new();
    metadata.insert(
        "chat_session_path".to_string(),
        chat_session_path.to_string(),
    );

    let context = PresetContext {
        agent_id: AgentId {
            tool: "github-copilot".to_string(),
            id: session_id.clone(),
            model: "unknown".to_string(),
        },
        external_session_id: session_id,
        trace_id: trace_id.to_string(),
        cwd: PathBuf::from(cwd),
        metadata,
    };

    let stream_source = Some(StreamSource {
        path: PathBuf::from(chat_session_path),
        format: StreamFormat::CopilotSessionJson,
        session_id: generate_session_id(&context.external_session_id, "github-copilot"),
        external_session_id: context.external_session_id.clone(),
        external_parent_session_id: None,
    });

    Ok(vec![ParsedHookEvent::PostFileEdit(PostFileEdit {
        context,
        file_paths: edited_filepaths,
        dirty_files,
        stream_source,
        tool_use_id: None,
    })])
}

// ---------------------------------------------------------------------------
// VS Code native path (PreToolUse / PostToolUse)
// ---------------------------------------------------------------------------

pub(super) fn parse_vscode_native_hooks(
    data: &serde_json::Value,
    hook_event_name: &str,
    trace_id: &str,
) -> Result<Vec<ParsedHookEvent>, GitAiError> {
    let cwd = parse::optional_str_multi(data, &["cwd", "workspace_folder", "workspaceFolder"])
        .ok_or_else(|| GitAiError::PresetError("cwd not found in hook_input".to_string()))?;

    let dirty_files = super::dirty_files_from_hook_data(data, cwd);

    let session_id = super::extract_session_id(data);

    let tool_name =
        parse::optional_str_multi(data, &["tool_name", "toolName"]).unwrap_or("unknown");

    // Enforce tool filtering to avoid creating checkpoints for read/search tools
    if !is_supported_vscode_edit_tool_name(tool_name) {
        return Err(GitAiError::PresetError(format!(
            "Skipping VS Code hook for unsupported tool_name '{}' (non-edit tool).",
            tool_name
        )));
    }

    let tool_input = data.get("tool_input").or_else(|| data.get("toolInput"));
    let tool_response = data
        .get("tool_response")
        .or_else(|| data.get("toolResponse"));

    // Extract file paths from tool_input and tool_response only (not session-level data)
    let extracted_paths =
        super::extract_filepaths_from_vscode_hook_payload(tool_input, tool_response, cwd);

    let transcript_path = transcript_path_from_hook_data(data).map(|s| s.to_string());

    if let Some(ref path) = transcript_path
        && looks_like_claude_transcript_path(path)
    {
        return Err(GitAiError::PresetError(
            "Skipping VS Code hook because transcript_path looks like a Claude transcript path."
                .to_string(),
        ));
    }

    if !is_likely_copilot_native_hook(transcript_path.as_deref()) {
        return Err(GitAiError::PresetError(format!(
            "Skipping VS Code hook for non-Copilot session (tool_name: {}).",
            tool_name,
        )));
    }

    let tool_class = classify_copilot_tool(tool_name);
    let is_bash = tool_class == ToolClass::Bash;
    let bash_command = parse::bash_command_from_hook_input(data);

    let tool_use_id = parse::optional_str_multi(data, &["tool_use_id", "toolUseId"])
        .unwrap_or("unknown")
        .to_string();

    let mut metadata = HashMap::new();
    if let Some(ref path) = transcript_path {
        metadata.insert("transcript_path".to_string(), path.clone());
        metadata.insert("chat_session_path".to_string(), path.clone());
    }

    // Determine transcript format: newer native uses EventStreamJsonl
    let transcript_format = super::transcript_format(transcript_path.as_deref().unwrap());

    let context = PresetContext {
        agent_id: AgentId {
            tool: "github-copilot".to_string(),
            id: session_id.clone(),
            model: "unknown".to_string(),
        },
        external_session_id: session_id,
        trace_id: trace_id.to_string(),
        cwd: PathBuf::from(cwd),
        metadata,
    };

    let stream_source = transcript_path.map(|tp| StreamSource {
        path: PathBuf::from(tp),
        format: transcript_format,
        session_id: generate_session_id(&context.external_session_id, "github-copilot"),
        external_session_id: context.external_session_id.clone(),
        external_parent_session_id: None,
    });

    if hook_event_name == "PreToolUse" {
        if is_bash {
            return Ok(vec![ParsedHookEvent::PreBashCall(PreBashCall {
                context,
                tool_use_id,
                command: bash_command,
            })]);
        }

        if tool_name.eq_ignore_ascii_case("create_file") {
            if extracted_paths.is_empty() {
                return Err(GitAiError::PresetError(
                    "No file path found in create_file PreToolUse tool_input".to_string(),
                ));
            }

            let mut empty_dirty_files: HashMap<PathBuf, String> = HashMap::new();
            for path in &extracted_paths {
                empty_dirty_files.insert(path.clone(), String::new());
            }
            return Ok(vec![ParsedHookEvent::PreFileEdit(PreFileEdit {
                context,
                file_paths: extracted_paths,
                dirty_files: Some(empty_dirty_files),
                tool_use_id: Some(tool_use_id),
            })]);
        }

        if extracted_paths.is_empty() {
            return Err(GitAiError::PresetError(format!(
                "No editable file paths found in VS Code hook input (tool_name: {}). Skipping checkpoint.",
                tool_name
            )));
        }

        return Ok(vec![ParsedHookEvent::PreFileEdit(PreFileEdit {
            context,
            file_paths: extracted_paths,
            dirty_files,
            tool_use_id: Some(tool_use_id),
        })]);
    }

    // PostToolUse
    if is_bash {
        return Ok(vec![ParsedHookEvent::PostBashCall(PostBashCall {
            context,
            tool_use_id,
            command: bash_command,
            stream_source,
        })]);
    }

    if extracted_paths.is_empty() {
        return Err(GitAiError::PresetError(format!(
            "No editable file paths found in VS Code PostToolUse hook input (tool_name: {}). Skipping checkpoint.",
            tool_name
        )));
    }

    Ok(vec![ParsedHookEvent::PostFileEdit(PostFileEdit {
        context,
        file_paths: extracted_paths,
        dirty_files,
        stream_source,
        tool_use_id: Some(tool_use_id),
    })])
}

// ---------------------------------------------------------------------------
// IDE-specific helpers
// ---------------------------------------------------------------------------

pub(super) fn transcript_path_from_hook_data(data: &serde_json::Value) -> Option<&str> {
    parse::optional_str_multi(
        data,
        &[
            "transcript_path",
            "transcriptPath",
            "chat_session_path",
            "chatSessionPath",
        ],
    )
}

fn looks_like_claude_transcript_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.contains("/.claude/") || normalized.contains("/claude/projects/")
}

fn looks_like_copilot_transcript_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.contains("/github.copilot-chat/transcripts/")
        || normalized.contains("vscode-chat-session")
        || normalized.contains("copilot_session")
        || (normalized.contains("/workspacestorage/") && normalized.contains("/chatsessions/"))
}

fn is_likely_copilot_native_hook(transcript_path: Option<&str>) -> bool {
    let Some(path) = transcript_path else {
        return false;
    };
    if looks_like_claude_transcript_path(path) {
        return false;
    }
    looks_like_copilot_transcript_path(path)
}

fn is_supported_vscode_edit_tool_name(tool_name: &str) -> bool {
    let lower = tool_name.to_ascii_lowercase();

    // Explicit bash/terminal tools
    let bash_tools = ["run_in_terminal"];
    if bash_tools.iter().any(|name| lower == *name) {
        return true;
    }

    let non_edit_keywords = [
        "find", "search", "read", "grep", "glob", "list", "ls", "fetch", "web", "open", "todo",
    ];
    if non_edit_keywords.iter().any(|kw| lower.contains(kw)) {
        return false;
    }

    let exact_edit_tools = [
        "write",
        "edit",
        "multiedit",
        "applypatch",
        "apply_patch",
        "copilot_insertedit",
        "copilot_replacestring",
        "vscode_editfile_internal",
        "create_file",
        "delete_file",
        "rename_file",
        "move_file",
        "replace_string_in_file",
        "insert_edit_into_file",
    ];
    if exact_edit_tools.iter().any(|name| lower == *name) {
        return true;
    }

    lower.contains("edit") || lower.contains("write") || lower.contains("replace")
}

/// Classify GitHub Copilot tool for bash vs file edit handling.
/// GithubCopilot is not in the `Agent` enum, so we implement classification locally.
fn classify_copilot_tool(tool_name: &str) -> ToolClass {
    let lower = tool_name.to_ascii_lowercase();
    match lower.as_str() {
        "run_in_terminal" => ToolClass::Bash,
        "create_file"
        | "replace_string_in_file"
        | "apply_patch"
        | "delete_file"
        | "rename_file"
        | "move_file" => ToolClass::FileEdit,
        _ if lower.contains("edit") || lower.contains("write") || lower.contains("replace") => {
            ToolClass::FileEdit
        }
        _ => ToolClass::Skip,
    }
}

#[cfg(test)]
mod tests;
