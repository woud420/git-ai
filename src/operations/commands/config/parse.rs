use crate::config::{AuthorConfig, CodexHooksFormat, NotesBackendKind};
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn parse_key_path(key: &str) -> Vec<String> {
    key.split('.').map(|s| s.to_string()).collect()
}

pub(super) fn parse_git_ai_hooks_object(
    value: &str,
) -> Result<HashMap<String, Vec<String>>, String> {
    let parsed: Value =
        serde_json::from_str(value).map_err(|e| format!("Invalid JSON for git_ai_hooks: {}", e))?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| "git_ai_hooks must be a JSON object".to_string())?;

    let mut hooks = HashMap::new();
    for (hook_name, commands_value) in obj {
        let name = hook_name.trim();
        if name.is_empty() {
            return Err("git_ai_hooks contains an empty hook name".to_string());
        }
        let commands = parse_hook_commands_value(commands_value)?;
        hooks.insert(name.to_string(), commands);
    }
    Ok(hooks)
}

/// Parse a JSON object of custom telemetry attributes.
///
/// String/number/bool values are coerced to strings using the same rules as the
/// `GIT_AI_CUSTOM_ATTRIBUTES` env var override (see `build_custom_attributes`).
/// Unlike the env path, which silently drops non-scalar values, the CLI rejects
/// them so a malformed `config set` fails loudly rather than persisting a
/// partially-applied object.
pub(super) fn parse_custom_attributes_object(
    value: &str,
) -> Result<HashMap<String, String>, String> {
    let parsed: Value = serde_json::from_str(value)
        .map_err(|e| format!("Invalid JSON for custom_attributes: {}", e))?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| "custom_attributes must be a JSON object".to_string())?;

    let mut attrs = HashMap::new();
    for (attr_name, attr_value) in obj {
        let name = attr_name.trim();
        if name.is_empty() {
            return Err("custom_attributes contains an empty attribute name".to_string());
        }
        let coerced = match attr_value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => {
                return Err(format!(
                    "custom_attributes value for '{}' must be a string, number, or boolean",
                    name
                ));
            }
        };
        attrs.insert(name.to_string(), coerced);
    }
    Ok(attrs)
}

pub(super) fn parse_hook_command_values(value: &str) -> Result<Vec<String>, String> {
    if let Ok(parsed) = serde_json::from_str::<Value>(value)
        && (parsed.is_string() || parsed.is_array())
    {
        return parse_hook_commands_value(&parsed);
    }

    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Hook command cannot be empty".to_string());
    }
    Ok(vec![trimmed.to_string()])
}

pub(super) fn parse_hook_commands_value(value: &Value) -> Result<Vec<String>, String> {
    match value {
        Value::String(command) => {
            let trimmed = command.trim();
            if trimmed.is_empty() {
                return Err("Hook command cannot be empty".to_string());
            }
            Ok(vec![trimmed.to_string()])
        }
        Value::Array(items) => {
            let mut commands = Vec::new();
            for item in items {
                let command = item.as_str().ok_or_else(|| {
                    "git_ai_hooks hook values must be a string or an array of strings".to_string()
                })?;
                let trimmed = command.trim();
                if trimmed.is_empty() {
                    return Err("Hook command cannot be empty".to_string());
                }
                commands.push(trimmed.to_string());
            }
            if commands.is_empty() {
                return Err("Hook command array cannot be empty".to_string());
            }
            Ok(commands)
        }
        _ => Err("git_ai_hooks hook values must be a string or an array of strings".to_string()),
    }
}

pub(super) fn parse_bool(value: &str) -> Result<bool, String> {
    match value.to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!(
            "Invalid boolean value: '{}'. Expected true/false",
            value
        )),
    }
}

pub(super) fn parse_value(value: &str) -> Result<Value, String> {
    // Try to parse as JSON first
    if let Ok(json_value) = serde_json::from_str::<Value>(value) {
        return Ok(json_value);
    }

    // Otherwise treat as string
    Ok(Value::String(value.to_string()))
}

pub(super) fn parse_author_config_object(value: &str) -> Result<AuthorConfig, String> {
    let parsed: Value =
        serde_json::from_str(value).map_err(|e| format!("Invalid JSON for author: {}", e))?;
    if !parsed.is_object() {
        return Err("author must be a JSON object".to_string());
    }

    serde_json::from_value::<AuthorConfig>(parsed)
        .map(AuthorConfig::normalized)
        .map_err(|e| format!("Invalid author config: {}", e))
}

/// Mask an API key for display (show first 4 and last 4 chars if long enough)
pub(super) fn mask_api_key(key: &str) -> String {
    if key.len() > 8 {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    } else {
        "****".to_string()
    }
}

/// Parse notes backend kind from a string value
pub(super) fn parse_notes_backend_kind(value: &str) -> Result<NotesBackendKind, String> {
    match value.trim().to_lowercase().as_str() {
        "git_notes" | "git-notes" => Ok(NotesBackendKind::GitNotes),
        "http" => Ok(NotesBackendKind::Http),
        _ => Err(format!(
            "Invalid notes_backend.kind '{}'. Expected 'git_notes' or 'http'",
            value
        )),
    }
}

pub(super) fn parse_codex_hooks_format(value: &str) -> Result<CodexHooksFormat, String> {
    match value.trim().to_lowercase().as_str() {
        "config_toml" | "config-toml" => Ok(CodexHooksFormat::ConfigToml),
        "hooks_json" | "hooks-json" => Ok(CodexHooksFormat::HooksJson),
        _ => Err(format!(
            "Invalid codex_hooks_format '{}'. Expected 'config_toml' or 'hooks_json'",
            value
        )),
    }
}

/// Validate prompt_storage value
pub(super) fn validate_prompt_storage_value(value: &str) -> Result<(), String> {
    if value != "default" && value != "notes" && value != "local" {
        return Err(format!(
            "Invalid prompt_storage value '{}'. Expected 'default', 'notes', or 'local'",
            value
        ));
    }
    Ok(())
}
