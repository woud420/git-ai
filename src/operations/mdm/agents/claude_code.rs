use crate::error::GitAiError;
use crate::operations::mdm::editor_cli::binary_exists;
use crate::operations::mdm::hook_installer::{HookCheckResult, HookInstaller, HookInstallerParams};
#[cfg(test)]
use crate::operations::mdm::hooks_merge::is_git_ai_checkpoint_command;
use crate::operations::mdm::hooks_merge::{
    MissingBehavior, catch_all_hook_status, edit_settings_json, install_catch_all_hooks,
    uninstall_catch_all_hooks,
};
use crate::operations::mdm::paths::{claude_config_dir, normalize_windows_path_for_shell};
use crate::operations::mdm::version::{
    MIN_CLAUDE_VERSION, get_binary_version, parse_version, version_meets_requirement,
};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

// Command patterns for hooks
const CLAUDE_PRE_TOOL_CMD: &str = "checkpoint claude --hook-input stdin";
const CLAUDE_POST_TOOL_CMD: &str = "checkpoint claude --hook-input stdin";
#[cfg(test)]
const CLAUDE_CATCH_ALL_MATCHER: &str = "*";

pub struct ClaudeCodeInstaller;

impl ClaudeCodeInstaller {
    fn settings_path() -> PathBuf {
        claude_config_dir().join("settings.json")
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
        let binary_path_str = normalize_windows_path_for_shell(&params.binary_path);
        let pre_tool_cmd = format!("{} {}", binary_path_str, CLAUDE_PRE_TOOL_CMD);
        let post_tool_cmd = format!("{} {}", binary_path_str, CLAUDE_POST_TOOL_CMD);

        edit_settings_json(
            settings_path,
            dry_run,
            MissingBehavior::TreatAsEmpty,
            |content| Ok(serde_json::from_str(content)?),
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
                if uninstall_catch_all_hooks(&mut hooks_obj, &["PreToolUse", "PostToolUse"])
                    && let Some(root) = merged.as_object_mut()
                {
                    root.insert("hooks".to_string(), hooks_obj);
                }
            },
        )
    }
}

impl HookInstaller for ClaudeCodeInstaller {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn id(&self) -> &str {
        "claude-code"
    }

    fn check_hooks(&self, _params: &HookInstallerParams) -> Result<HookCheckResult, GitAiError> {
        let has_binary = binary_exists("claude");
        let has_dotfiles = claude_config_dir().exists();

        if !has_binary && !has_dotfiles {
            return Ok(HookCheckResult::tool_not_installed());
        }

        if has_binary
            && let Ok(version_str) = get_binary_version("claude")
            && let Some(version) = parse_version(&version_str)
            && !version_meets_requirement(version, MIN_CLAUDE_VERSION)
        {
            return Err(GitAiError::Generic(format!(
                "Claude Code version {}.{} detected, but minimum version {}.{} is required",
                version.0, version.1, MIN_CLAUDE_VERSION.0, MIN_CLAUDE_VERSION.1
            )));
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

    fn process_names(&self) -> Vec<&str> {
        vec!["claude"]
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
