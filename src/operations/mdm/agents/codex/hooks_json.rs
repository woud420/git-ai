use crate::error::GitAiError;
use serde_json::{Value as JsonValue, json};
use std::path::Path;

use super::{CODEX_HOOK_EVENTS, CodexInstaller};

impl CodexInstaller {
    pub(super) fn parse_hooks_json(content: &str) -> Result<JsonValue, GitAiError> {
        if content.trim().is_empty() {
            return Ok(json!({}));
        }

        let parsed: JsonValue = serde_json::from_str(content)?;
        if !parsed.is_object() {
            return Err(GitAiError::Generic(
                "Codex hooks.json root must be a JSON object".to_string(),
            ));
        }
        Ok(parsed)
    }

    pub(super) fn remove_codex_hooks_from_json(
        hooks_json: &JsonValue,
    ) -> Result<(JsonValue, bool), GitAiError> {
        let mut merged = hooks_json.clone();
        let root = merged.as_object_mut().ok_or_else(|| {
            GitAiError::Generic("Codex hooks.json root must be a JSON object".to_string())
        })?;
        let Some(hooks_entry) = root.get_mut("hooks") else {
            return Ok((merged, false));
        };
        if !hooks_entry.is_object() {
            return Ok((merged, false));
        }
        let hooks_obj = hooks_entry.as_object_mut().ok_or_else(|| {
            GitAiError::Generic("Codex hooks field must be a JSON object".to_string())
        })?;

        let mut changed = false;
        for event_name in CODEX_HOOK_EVENTS {
            let Some(blocks) = hooks_obj.get(event_name).and_then(|value| value.as_array()) else {
                continue;
            };

            let mut cleaned_blocks = Vec::new();
            for block in blocks.clone() {
                let mut cleaned_block = block;
                let original_hook_count = cleaned_block
                    .get("hooks")
                    .and_then(|value| value.as_array())
                    .map(|hooks| hooks.len())
                    .unwrap_or(0);
                let cleaned_hooks = cleaned_block
                    .get("hooks")
                    .and_then(|value| value.as_array())
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|hook| {
                        hook.get("command")
                            .and_then(|value| value.as_str())
                            .map(|cmd| !Self::is_git_ai_codex_command(cmd))
                            .unwrap_or(true)
                    })
                    .collect::<Vec<_>>();
                if cleaned_hooks.len() != original_hook_count {
                    changed = true;
                }

                if let Some(block_obj) = cleaned_block.as_object_mut() {
                    block_obj.insert("hooks".to_string(), JsonValue::Array(cleaned_hooks));
                }

                let remaining_hook_count = cleaned_block
                    .get("hooks")
                    .and_then(|value| value.as_array())
                    .map(|hooks| hooks.len())
                    .unwrap_or(0);
                if remaining_hook_count > 0 {
                    cleaned_blocks.push(cleaned_block);
                }
            }

            if cleaned_blocks.is_empty() {
                hooks_obj.remove(event_name);
            } else {
                hooks_obj.insert(event_name.to_string(), JsonValue::Array(cleaned_blocks));
            }
        }

        Ok((merged, changed))
    }

    pub(super) fn hooks_json_with_installed_hooks(
        hooks_json: &JsonValue,
        binary_path: &Path,
    ) -> Result<JsonValue, GitAiError> {
        let (mut merged, _) = Self::remove_codex_hooks_from_json(hooks_json)?;
        let root = merged.as_object_mut().ok_or_else(|| {
            GitAiError::Generic("Codex hooks.json root must be a JSON object".to_string())
        })?;
        let hooks_entry = root.entry("hooks").or_insert_with(|| json!({}));
        if !hooks_entry.is_object() {
            *hooks_entry = json!({});
        }
        let hooks_obj = hooks_entry.as_object_mut().ok_or_else(|| {
            GitAiError::Generic("Codex hooks field must be a JSON object".to_string())
        })?;

        let desired_command = Self::desired_command(binary_path);
        for event_name in CODEX_HOOK_EVENTS {
            let mut blocks = hooks_obj
                .get(event_name)
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();

            let target_idx = blocks
                .iter()
                .position(|block| block.get("matcher").is_none())
                .unwrap_or_else(|| {
                    blocks.push(json!({ "hooks": [] }));
                    blocks.len() - 1
                });

            if !blocks[target_idx].is_object() {
                blocks[target_idx] = json!({ "hooks": [] });
            }
            let block_obj = blocks[target_idx].as_object_mut().ok_or_else(|| {
                GitAiError::Generic("Codex hooks.json hook block must be an object".to_string())
            })?;
            let hooks = block_obj.entry("hooks").or_insert_with(|| json!([]));
            if !hooks.is_array() {
                *hooks = json!([]);
            }
            let hooks_array = hooks.as_array_mut().ok_or_else(|| {
                GitAiError::Generic("Codex hooks.json hooks entry must be an array".to_string())
            })?;
            hooks_array.push(json!({
                "type": "command",
                "command": desired_command.clone(),
            }));

            hooks_obj.insert(event_name.to_string(), JsonValue::Array(blocks));
        }

        Ok(merged)
    }

    pub(super) fn hooks_json_has_any_entries(hooks_json: &JsonValue) -> bool {
        hooks_json
            .get("hooks")
            .and_then(|value| value.as_object())
            .map(|hooks| {
                hooks.values().any(|value| {
                    value
                        .as_array()
                        .map(|blocks| !blocks.is_empty())
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }

    pub(super) fn hooks_json_has_git_ai_entries(hooks_json: &JsonValue) -> bool {
        CODEX_HOOK_EVENTS.iter().any(|event_name| {
            hooks_json
                .get("hooks")
                .and_then(|hooks| hooks.get(*event_name))
                .and_then(|value| value.as_array())
                .map(|blocks| {
                    blocks.iter().any(|block| {
                        block
                            .get("hooks")
                            .and_then(|value| value.as_array())
                            .map(|hooks| {
                                hooks.iter().any(|hook| {
                                    hook.get("command")
                                        .and_then(|value| value.as_str())
                                        .map(Self::is_git_ai_codex_command)
                                        .unwrap_or(false)
                                })
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        })
    }
}
