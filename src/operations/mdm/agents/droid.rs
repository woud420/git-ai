use crate::error::GitAiError;
use crate::operations::mdm::editor_cli::binary_exists;
use crate::operations::mdm::hook_installer::{HookCheckResult, HookInstaller, HookInstallerParams};
#[cfg(test)]
use crate::operations::mdm::hooks_merge::is_git_ai_checkpoint_command;
use crate::operations::mdm::hooks_merge::{
    MissingBehavior, catch_all_hook_status, edit_settings_json, install_catch_all_hooks,
    uninstall_catch_all_hooks,
};
use crate::operations::mdm::paths::home_dir;
use jsonc_parser::ParseOptions;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

/// Droid's settings.json uses JSONC (JSON with `//` line comments, `/* */` block
/// comments, and trailing commas). Standard `serde_json` rejects these, so we
/// parse through `jsonc_parser` first and convert to `serde_json::Value`.
/// NOTE: This parse-to-serde-Value approach discards JSONC comments and trailing
/// commas. If comment preservation becomes important, migrate to CstRootNode
/// (as used in utils.rs::update_vscode_chat_hook_settings).
fn parse_jsonc_settings(content: &str) -> Result<Value, GitAiError> {
    let parsed = jsonc_parser::parse_to_value(content, &ParseOptions::default())
        .map_err(|e| GitAiError::Generic(format!("Failed to parse Droid settings: {e}")))?;
    Ok(match parsed {
        Some(val) => jsonc_to_serde(val),
        None => json!({}),
    })
}

fn jsonc_to_serde(val: jsonc_parser::JsonValue<'_>) -> Value {
    match val {
        jsonc_parser::JsonValue::Null => Value::Null,
        jsonc_parser::JsonValue::Boolean(b) => Value::Bool(b),
        jsonc_parser::JsonValue::Number(n) => serde_json::from_str(n).unwrap_or(Value::Null),
        jsonc_parser::JsonValue::String(s) => Value::String(s.into_owned()),
        jsonc_parser::JsonValue::Array(arr) => {
            Value::Array(arr.into_iter().map(jsonc_to_serde).collect())
        }
        jsonc_parser::JsonValue::Object(obj) => {
            let map: serde_json::Map<String, Value> = obj
                .into_iter()
                .map(|(k, v)| (k, jsonc_to_serde(v)))
                .collect();
            Value::Object(map)
        }
    }
}

const DROID_PRE_TOOL_CMD: &str = "checkpoint droid --hook-input stdin";
const DROID_POST_TOOL_CMD: &str = "checkpoint droid --hook-input stdin";
#[cfg(test)]
const DROID_CATCH_ALL_MATCHER: &str = "*";

pub struct DroidInstaller;

impl DroidInstaller {
    fn settings_path() -> PathBuf {
        home_dir().join(".factory").join("settings.json")
    }

    /// Returns `(hooks_installed, hooks_up_to_date)` from a parsed settings value.
    /// `hooks_installed` = git-ai checkpoint command exists in ANY matcher block.
    /// `hooks_up_to_date` = git-ai checkpoint command exists in the `"*"` catch-all block.
    fn hook_status(settings: &Value) -> (bool, bool) {
        catch_all_hook_status(settings, "PreToolUse")
    }

    fn install_hooks_at(
        settings_path: &Path,
        params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        let binary_path = params.binary_path.to_string_lossy().to_string();
        let pre_tool_cmd = format!("{} {}", binary_path, DROID_PRE_TOOL_CMD);
        let post_tool_cmd = format!("{} {}", binary_path, DROID_POST_TOOL_CMD);

        edit_settings_json(
            settings_path,
            dry_run,
            MissingBehavior::TreatAsEmpty,
            parse_jsonc_settings,
            |merged| {
                let mut hooks_obj = merged.get("hooks").cloned().unwrap_or_else(|| json!({}));
                install_catch_all_hooks(
                    &mut hooks_obj,
                    &[
                        ("PreToolUse", pre_tool_cmd.as_str()),
                        ("PostToolUse", post_tool_cmd.as_str()),
                    ],
                );
                if let Some(root) = merged.as_object_mut() {
                    root.insert("hooks".to_string(), hooks_obj);
                }

                // Add claudeHooksImported flag if it doesn't exist.
                if let Some(hooks) = merged.get_mut("hooks").and_then(|h| h.as_object_mut())
                    && !hooks.contains_key("claudeHooksImported")
                {
                    hooks.insert("claudeHooksImported".to_string(), json!(true));
                }
            },
        )
    }

    fn uninstall_hooks_at(
        settings_path: &Path,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        edit_settings_json(
            settings_path,
            dry_run,
            MissingBehavior::NoOp,
            parse_jsonc_settings,
            |merged| {
                let Some(mut hooks_obj) = merged.get("hooks").cloned() else {
                    return;
                };
                if uninstall_catch_all_hooks(&mut hooks_obj, &["PreToolUse", "PostToolUse"])
                    && let Some(root) = merged.as_object_mut()
                {
                    root.insert("hooks".to_string(), hooks_obj);
                }
            },
        )
    }
}

impl HookInstaller for DroidInstaller {
    fn name(&self) -> &str {
        "Droid"
    }

    fn id(&self) -> &str {
        "droid"
    }

    fn process_names(&self) -> Vec<&str> {
        vec!["droid"]
    }

    fn check_hooks(&self, _params: &HookInstallerParams) -> Result<HookCheckResult, GitAiError> {
        let has_binary = binary_exists("droid");
        let has_dotfiles = home_dir().join(".factory").exists();

        if !has_binary && !has_dotfiles {
            return Ok(HookCheckResult::tool_not_installed());
        }

        let settings_path = Self::settings_path();
        if !settings_path.exists() {
            return Ok(HookCheckResult::installed_without_hooks());
        }

        let content = fs::read_to_string(&settings_path)?;
        let existing: Value = parse_jsonc_settings(&content).unwrap_or_else(|_| json!({}));
        let (hooks_installed, hooks_up_to_date) = Self::hook_status(&existing);

        Ok(HookCheckResult::installed(
            hooks_installed,
            hooks_up_to_date,
        ))
    }

    fn install_hooks(
        &self,
        params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        Self::install_hooks_at(&Self::settings_path(), params, dry_run)
    }

    fn uninstall_hooks(
        &self,
        _params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        Self::uninstall_hooks_at(&Self::settings_path(), dry_run)
    }
}

#[cfg(test)]
mod tests;
