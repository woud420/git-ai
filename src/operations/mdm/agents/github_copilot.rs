use crate::error::GitAiError;
use crate::operations::mdm::editor_cli::resolve_editor_cli;
use crate::operations::mdm::file_ops::{generate_diff, write_atomic};
use crate::operations::mdm::hook_installer::{HookCheckResult, HookInstaller, HookInstallerParams};
use crate::operations::mdm::paths::{home_dir, normalize_windows_path_for_shell};
use crate::operations::mdm::version::{
    MIN_CODE_VERSION, get_editor_version, parse_version, version_meets_requirement,
};
use crate::operations::mdm::vscode_settings::{
    settings_paths_for_products, should_process_settings_target,
};
use crate::process_spawn::quote_posix_shell_word;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

const GITHUB_COPILOT_PRE_TOOL_CMD: &str = "checkpoint github-copilot --hook-input stdin";
const GITHUB_COPILOT_POST_TOOL_CMD: &str = "checkpoint github-copilot --hook-input stdin";

pub struct GitHubCopilotInstaller;

impl GitHubCopilotInstaller {
    fn hooks_path() -> PathBuf {
        home_dir()
            .join(".copilot")
            .join("hooks")
            .join("git-ai.json")
    }

    fn legacy_hooks_path() -> PathBuf {
        home_dir().join(".github").join("hooks").join("git-ai.json")
    }

    fn settings_targets() -> Vec<PathBuf> {
        settings_paths_for_products(&["Code", "Code - Insiders"])
    }

    fn is_github_copilot_checkpoint_command(cmd: &str) -> bool {
        cmd.contains("git-ai checkpoint github-copilot")
            || (cmd.contains("git-ai")
                && cmd.contains("checkpoint")
                && cmd.contains("github-copilot"))
    }

    fn is_github_copilot_checkpoint_hook(hook: &Value) -> bool {
        ["command", "powershell"]
            .iter()
            .filter_map(|field| hook.get(*field).and_then(|value| value.as_str()))
            .any(Self::is_github_copilot_checkpoint_command)
    }

    fn checkpoint_hook(binary_path: &Path, checkpoint_command: &str) -> Value {
        let binary_path = normalize_windows_path_for_shell(binary_path);
        let shell_path = quote_posix_shell_word(&binary_path);
        let powershell_path = format!("'{}'", binary_path.replace('\'', "''"));

        json!({
            "type": "command",
            "command": format!("{} {}", shell_path, checkpoint_command),
            "powershell": format!("& {} {}", powershell_path, checkpoint_command),
        })
    }

    fn hook_has_desired_command(hook: &Value, desired_hook: &Value) -> bool {
        ["type", "command", "powershell"]
            .iter()
            .all(|field| hook.get(*field) == desired_hook.get(*field))
    }

    fn merge_checkpoint_hook(existing_hook: &Value, desired_hook: &Value) -> Value {
        let mut updated_hook = existing_hook.clone();
        let Some(updated_hook) = updated_hook.as_object_mut() else {
            return desired_hook.clone();
        };

        for field in ["type", "command", "powershell"] {
            if let Some(value) = desired_hook.get(field) {
                updated_hook.insert(field.to_string(), value.clone());
            }
        }

        Value::Object(updated_hook.clone())
    }
}

impl HookInstaller for GitHubCopilotInstaller {
    fn name(&self) -> &str {
        "GitHub Copilot"
    }

    fn id(&self) -> &str {
        "github-copilot"
    }

    fn process_names(&self) -> Vec<&str> {
        vec!["Code", "code"]
    }

    fn check_hooks(&self, params: &HookInstallerParams) -> Result<HookCheckResult, GitAiError> {
        let resolved_cli = resolve_editor_cli("code");
        let has_cli = resolved_cli.is_some();
        let has_vscode_dotfiles = home_dir().join(".vscode").exists();
        let has_copilot_dotfiles = home_dir().join(".copilot").exists();
        let has_github_dotfiles = home_dir().join(".github").exists();
        let has_settings_targets = Self::settings_targets()
            .iter()
            .any(|path| should_process_settings_target(path));

        if !has_cli
            && !has_vscode_dotfiles
            && !has_copilot_dotfiles
            && !has_github_dotfiles
            && !has_settings_targets
        {
            return Ok(HookCheckResult::tool_not_installed());
        }

        // If we have a CLI, check version.
        if let Some(cli) = &resolved_cli
            && let Ok(version_str) = get_editor_version(cli)
            && let Some(version) = parse_version(&version_str)
            && !version_meets_requirement(version, MIN_CODE_VERSION)
        {
            return Err(GitAiError::Generic(format!(
                "VS Code version {}.{} detected, but minimum version {}.{} is required",
                version.0, version.1, MIN_CODE_VERSION.0, MIN_CODE_VERSION.1
            )));
        }

        let hooks_path = Self::hooks_path();
        let legacy_path = Self::legacy_hooks_path();
        if !hooks_path.exists() && !legacy_path.exists() {
            return Ok(HookCheckResult::installed_without_hooks());
        }

        if !hooks_path.exists() && legacy_path.exists() {
            return Ok(HookCheckResult::installed(true, false));
        }

        let content = fs::read_to_string(&hooks_path)?;
        let existing: Value = serde_json::from_str(&content).unwrap_or_else(|_| json!({}));

        let pre_desired = Self::checkpoint_hook(&params.binary_path, GITHUB_COPILOT_PRE_TOOL_CMD);
        let post_desired = Self::checkpoint_hook(&params.binary_path, GITHUB_COPILOT_POST_TOOL_CMD);

        let has_pre_installed = existing
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(Self::is_github_copilot_checkpoint_hook))
            .unwrap_or(false);

        let has_post_installed = existing
            .get("hooks")
            .and_then(|h| h.get("PostToolUse"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(Self::is_github_copilot_checkpoint_hook))
            .unwrap_or(false);

        let has_pre_up_to_date = existing
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|hook| Self::hook_has_desired_command(hook, &pre_desired))
            })
            .unwrap_or(false);

        let has_post_up_to_date = existing
            .get("hooks")
            .and_then(|h| h.get("PostToolUse"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|hook| Self::hook_has_desired_command(hook, &post_desired))
            })
            .unwrap_or(false);

        Ok(HookCheckResult::installed(
            has_pre_installed || has_post_installed,
            has_pre_up_to_date && has_post_up_to_date,
        ))
    }

    fn install_hooks(
        &self,
        params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        let hooks_path = Self::hooks_path();

        if !dry_run && let Some(dir) = hooks_path.parent() {
            fs::create_dir_all(dir)?;
        }

        let existing_content = if hooks_path.exists() {
            fs::read_to_string(&hooks_path)?
        } else {
            String::new()
        };

        let existing: Value = if existing_content.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&existing_content)?
        };

        let desired: Value = json!({
            "hooks": {
                "PreToolUse": [
                    Self::checkpoint_hook(&params.binary_path, GITHUB_COPILOT_PRE_TOOL_CMD)
                ],
                "PostToolUse": [
                    Self::checkpoint_hook(&params.binary_path, GITHUB_COPILOT_POST_TOOL_CMD)
                ]
            }
        });

        let mut merged = existing.clone();
        if !merged.is_object() {
            merged = json!({});
        }

        let mut hooks_obj = match merged.get("hooks") {
            Some(v) if v.is_object() => v.clone(),
            Some(_) => json!({}),
            None => json!({}),
        };

        for hook_name in &["PreToolUse", "PostToolUse"] {
            let desired_hook = desired
                .get("hooks")
                .and_then(|h| h.get(*hook_name))
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .cloned();
            let Some(desired_hook) = desired_hook else {
                continue;
            };

            let mut existing_hooks = hooks_obj
                .get(*hook_name)
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut found_idx = None;
            let mut needs_update = false;

            for (idx, existing_hook) in existing_hooks.iter().enumerate() {
                if Self::is_github_copilot_checkpoint_hook(existing_hook) && found_idx.is_none() {
                    found_idx = Some(idx);
                    if !Self::hook_has_desired_command(existing_hook, &desired_hook) {
                        needs_update = true;
                    }
                }
            }

            match found_idx {
                Some(idx) => {
                    if needs_update {
                        existing_hooks[idx] =
                            Self::merge_checkpoint_hook(&existing_hooks[idx], &desired_hook);
                    }

                    let keep_idx = idx;
                    let mut current_idx = 0;
                    existing_hooks.retain(|hook| {
                        if current_idx == keep_idx {
                            current_idx += 1;
                            true
                        } else if Self::is_github_copilot_checkpoint_hook(hook) {
                            current_idx += 1;
                            false
                        } else {
                            current_idx += 1;
                            true
                        }
                    });
                }
                None => existing_hooks.push(desired_hook.clone()),
            }

            if let Some(obj) = hooks_obj.as_object_mut() {
                obj.insert(hook_name.to_string(), Value::Array(existing_hooks));
            }
        }

        if let Some(root) = merged.as_object_mut() {
            root.insert("hooks".to_string(), hooks_obj);
        }

        if !dry_run {
            let legacy_path = Self::legacy_hooks_path();
            if legacy_path.exists() {
                let _ = fs::remove_file(&legacy_path);
            }
        }

        if existing == merged {
            return Ok(None);
        }

        let new_content = serde_json::to_string_pretty(&merged)?;
        let diff_output = generate_diff(&hooks_path, &existing_content, &new_content);

        if !dry_run {
            write_atomic(&hooks_path, new_content.as_bytes())?;
        }

        Ok(Some(diff_output))
    }

    fn uninstall_hooks(
        &self,
        _params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        if !dry_run {
            let legacy_path = Self::legacy_hooks_path();
            if legacy_path.exists() {
                let _ = fs::remove_file(&legacy_path);
            }
        }

        let hooks_path = Self::hooks_path();

        if !hooks_path.exists() {
            return Ok(None);
        }

        let existing_content = fs::read_to_string(&hooks_path)?;
        let existing: Value = serde_json::from_str(&existing_content)?;

        let mut merged = existing.clone();
        let mut hooks_obj = match merged.get("hooks").cloned() {
            Some(h) => h,
            None => return Ok(None),
        };

        let mut changed = false;

        for hook_name in &["PreToolUse", "PostToolUse"] {
            if let Some(hooks_array) = hooks_obj.get_mut(*hook_name).and_then(|v| v.as_array_mut())
            {
                let original_len = hooks_array.len();
                hooks_array.retain(|hook| !Self::is_github_copilot_checkpoint_hook(hook));
                if hooks_array.len() != original_len {
                    changed = true;
                }
            }
        }

        if !changed {
            return Ok(None);
        }

        if let Some(root) = merged.as_object_mut() {
            root.insert("hooks".to_string(), hooks_obj);
        }

        let new_content = serde_json::to_string_pretty(&merged)?;
        let diff_output = generate_diff(&hooks_path, &existing_content, &new_content);

        if !dry_run {
            write_atomic(&hooks_path, new_content.as_bytes())?;
        }

        Ok(Some(diff_output))
    }
}

#[cfg(test)]
mod tests;
