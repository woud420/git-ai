use crate::error::GitAiError;
use crate::operations::mdm::editor_cli::resolve_editor_cli;
use crate::operations::mdm::hook_installer::{
    HookCheckResult, HookInstaller, HookInstallerParams, InstallResult,
};
use crate::operations::mdm::hooks_merge::{
    MissingBehavior, edit_settings_json, parse_json, upsert_singleton_hook,
};
use crate::operations::mdm::hooks_merge_flat::has_flat_hook;
use crate::operations::mdm::paths::{home_dir, normalize_windows_path_for_shell};
use crate::operations::mdm::version::{
    MIN_CODE_VERSION, get_editor_version, parse_version, version_meets_requirement,
};
use crate::operations::mdm::vscode_settings::{
    settings_paths_for_products, should_process_settings_target, update_vscode_chat_hook_settings,
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

        let installed = |hook_name| {
            has_flat_hook(
                &existing,
                hook_name,
                Self::is_github_copilot_checkpoint_hook,
            )
        };
        let up_to_date = |hook_name, desired: &Value| {
            has_flat_hook(&existing, hook_name, |hook| {
                Self::hook_has_desired_command(hook, desired)
            })
        };

        Ok(HookCheckResult::installed(
            installed("PreToolUse") || installed("PostToolUse"),
            up_to_date("PreToolUse", &pre_desired) && up_to_date("PostToolUse", &post_desired),
        ))
    }

    fn install_hooks(
        &self,
        params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        let desired_hooks = [
            (
                "PreToolUse",
                Self::checkpoint_hook(&params.binary_path, GITHUB_COPILOT_PRE_TOOL_CMD),
            ),
            (
                "PostToolUse",
                Self::checkpoint_hook(&params.binary_path, GITHUB_COPILOT_POST_TOOL_CMD),
            ),
        ];

        let diff = edit_settings_json(
            &Self::hooks_path(),
            dry_run,
            MissingBehavior::TreatAsEmpty,
            parse_json,
            |merged| {
                if !merged.is_object() {
                    *merged = json!({});
                }
                let mut hooks_obj = match merged.get("hooks") {
                    Some(hooks) if hooks.is_object() => hooks.clone(),
                    _ => json!({}),
                };

                for (hook_name, desired_hook) in desired_hooks {
                    let mut existing_hooks = hooks_obj
                        .get(hook_name)
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();

                    upsert_singleton_hook(
                        &mut existing_hooks,
                        desired_hook,
                        Self::is_github_copilot_checkpoint_hook,
                        |existing, desired| {
                            if !Self::hook_has_desired_command(existing, &desired) {
                                *existing = Self::merge_checkpoint_hook(existing, &desired);
                            }
                        },
                    );

                    if let Some(obj) = hooks_obj.as_object_mut() {
                        obj.insert(hook_name.to_string(), Value::Array(existing_hooks));
                    }
                }

                if let Some(root) = merged.as_object_mut() {
                    root.insert("hooks".to_string(), hooks_obj);
                }
            },
        )?;

        if !dry_run {
            let _ = fs::remove_file(Self::legacy_hooks_path());
        }

        Ok(diff)
    }

    fn uninstall_hooks(
        &self,
        _params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        if !dry_run {
            let _ = fs::remove_file(Self::legacy_hooks_path());
        }

        edit_settings_json(
            &Self::hooks_path(),
            dry_run,
            MissingBehavior::NoOp,
            parse_json,
            |merged| {
                for hook_name in ["PreToolUse", "PostToolUse"] {
                    if let Some(hooks) = merged
                        .get_mut("hooks")
                        .and_then(|hooks| hooks.get_mut(hook_name))
                        .and_then(|hooks| hooks.as_array_mut())
                    {
                        hooks.retain(|hook| !Self::is_github_copilot_checkpoint_hook(hook));
                    }
                }
            },
        )
    }

    fn install_extras(
        &self,
        _params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Vec<InstallResult>, GitAiError> {
        Ok(Self::settings_targets()
            .into_iter()
            .filter(|path| should_process_settings_target(path))
            .map(
                |path| match update_vscode_chat_hook_settings(&path, dry_run) {
                    Ok(diff) => InstallResult {
                        changed: diff.is_some(),
                        diff,
                        message: format!(
                            "GitHub Copilot: chat hook settings configured in {}",
                            path.display()
                        ),
                    },
                    Err(error) => InstallResult {
                        changed: false,
                        diff: None,
                        message: format!(
                            "GitHub Copilot: Failed to configure chat hook settings in {}: {error}",
                            path.display()
                        ),
                    },
                },
            )
            .collect())
    }
}

#[cfg(test)]
#[path = "github_copilot_tests.rs"]
mod tests;
