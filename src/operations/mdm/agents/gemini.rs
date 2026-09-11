use crate::error::GitAiError;
use crate::operations::mdm::editor_cli::binary_exists;
use crate::operations::mdm::hook_installer::{HookCheckResult, HookInstaller, HookInstallerParams};
#[cfg(test)]
use crate::operations::mdm::hooks_merge::is_git_ai_checkpoint_command;
use crate::operations::mdm::hooks_merge::{
    MissingBehavior, catch_all_hook_status, edit_settings_json, install_catch_all_hooks,
    uninstall_catch_all_hooks,
};
use crate::operations::mdm::paths::gemini_config_dir;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

// Command patterns for hooks
const GEMINI_BEFORE_TOOL_CMD: &str = "checkpoint gemini --hook-input stdin";
const GEMINI_AFTER_TOOL_CMD: &str = "checkpoint gemini --hook-input stdin";
#[cfg(test)]
const GEMINI_CATCH_ALL_MATCHER: &str = "*";

pub struct GeminiInstaller;

impl GeminiInstaller {
    fn settings_path() -> PathBuf {
        gemini_config_dir().join("settings.json")
    }

    /// Returns `(hooks_installed, hooks_up_to_date)` from a parsed settings value.
    /// `hooks_installed` = git-ai checkpoint command exists in ANY matcher block.
    /// `hooks_up_to_date` = git-ai checkpoint command exists in the `"*"` catch-all block.
    fn hook_status(settings: &Value) -> (bool, bool) {
        catch_all_hook_status(settings, "BeforeTool")
    }

    fn install_hooks_at(
        settings_path: &Path,
        params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        let before_tool_cmd = format!(
            "{} {}",
            params.binary_path.display(),
            GEMINI_BEFORE_TOOL_CMD
        );
        let after_tool_cmd = format!("{} {}", params.binary_path.display(), GEMINI_AFTER_TOOL_CMD);

        edit_settings_json(
            settings_path,
            dry_run,
            MissingBehavior::TreatAsEmpty,
            |content| Ok(serde_json::from_str(content)?),
            |merged| {
                // Ensure tools.enableHooks is set to true.
                if let Some(tools_obj) = merged.get_mut("tools").and_then(|t| t.as_object_mut()) {
                    if tools_obj.get("enableHooks") != Some(&json!(true)) {
                        tools_obj.insert("enableHooks".to_string(), json!(true));
                    }
                } else if let Some(root) = merged.as_object_mut() {
                    root.insert("tools".to_string(), json!({ "enableHooks": true }));
                }

                let mut hooks_obj = merged.get("hooks").cloned().unwrap_or_else(|| json!({}));
                install_catch_all_hooks(
                    &mut hooks_obj,
                    &[
                        ("BeforeTool", before_tool_cmd.as_str()),
                        ("AfterTool", after_tool_cmd.as_str()),
                    ],
                );
                if let Some(root) = merged.as_object_mut() {
                    root.insert("hooks".to_string(), hooks_obj);
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
            |content| Ok(serde_json::from_str(content)?),
            |merged| {
                let Some(mut hooks_obj) = merged.get("hooks").cloned() else {
                    return;
                };
                if uninstall_catch_all_hooks(&mut hooks_obj, &["BeforeTool", "AfterTool"])
                    && let Some(root) = merged.as_object_mut()
                {
                    root.insert("hooks".to_string(), hooks_obj);
                }
            },
        )
    }
}

impl HookInstaller for GeminiInstaller {
    fn name(&self) -> &str {
        "Gemini"
    }

    fn id(&self) -> &str {
        "gemini"
    }

    fn process_names(&self) -> Vec<&str> {
        vec!["gemini"]
    }

    fn check_hooks(&self, _params: &HookInstallerParams) -> Result<HookCheckResult, GitAiError> {
        let has_binary = binary_exists("gemini");
        let has_dotfiles = gemini_config_dir().exists();

        if !has_binary && !has_dotfiles {
            return Ok(HookCheckResult::tool_not_installed());
        }

        let settings_path = Self::settings_path();
        if !settings_path.exists() {
            return Ok(HookCheckResult::installed_without_hooks());
        }

        let content = fs::read_to_string(&settings_path)?;
        let existing: Value = serde_json::from_str(&content).unwrap_or_else(|_| json!({}));
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
