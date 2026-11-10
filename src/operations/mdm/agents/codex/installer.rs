use crate::config::{CodexHooksFormat, Config};
use crate::error::GitAiError;
use crate::operations::mdm::editor_cli::binary_exists;
use crate::operations::mdm::file_ops::{generate_diff, write_atomic};
use crate::operations::mdm::hook_installer::{HookCheckResult, HookInstaller, HookInstallerParams};
use crate::operations::mdm::paths::codex_home_dir;
use serde_json::{Value as JsonValue, json};
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;
use toml::map::Map;

use super::CodexInstaller;

// --- Path helpers and command construction ---

impl CodexInstaller {
    pub(super) fn config_path() -> PathBuf {
        codex_home_dir().join("config.toml")
    }

    pub(super) fn hooks_json_path() -> PathBuf {
        codex_home_dir().join("hooks.json")
    }

    pub(super) fn desired_command(binary_path: &Path) -> String {
        format!("{} {}", binary_path.display(), super::CODEX_CHECKPOINT_CMD)
    }
}

// --- Steps shared by check, install and uninstall ---

impl CodexInstaller {
    /// The `(config.toml, hooks.json)` an install in `HooksJson` format produces.
    fn hooks_json_format_desired_state(
        config: &TomlValue,
        hooks_json: &JsonValue,
        binary_path: &Path,
    ) -> Result<(TomlValue, JsonValue), GitAiError> {
        // config_with_hooks_feature_enabled also drops a git-ai `notify`.
        let (config_without_inline_hooks, _) = Self::remove_inline_hooks_from_config(config)?;
        Ok((
            Self::config_with_hooks_feature_enabled(&config_without_inline_hooks)?,
            Self::hooks_json_with_installed_hooks(hooks_json, binary_path)?,
        ))
    }

    /// Write `new_content` to `path` unless `dry_run`; returns its diff.
    fn write_with_diff(
        path: &Path,
        old_content: &str,
        new_content: &str,
        dry_run: bool,
    ) -> Result<String, GitAiError> {
        if !dry_run {
            write_atomic(path, new_content.as_bytes())?;
        }
        Ok(generate_diff(path, old_content, new_content))
    }

    fn write_config(
        config: &TomlValue,
        old_content: &str,
        dry_run: bool,
    ) -> Result<String, GitAiError> {
        let new_content = toml::to_string_pretty(config).map_err(|e| {
            GitAiError::Generic(format!("Failed to serialize Codex config.toml: {e}"))
        })?;
        Self::write_with_diff(&Self::config_path(), old_content, &new_content, dry_run)
    }

    /// Write a hooks.json that had its git-ai entries stripped, deleting the
    /// file instead once no hooks are left in it.
    fn write_cleaned_hooks_json(
        cleaned_hooks: &JsonValue,
        old_content: &str,
        dry_run: bool,
    ) -> Result<String, GitAiError> {
        let path = Self::hooks_json_path();
        if Self::hooks_json_has_any_entries(cleaned_hooks) {
            let new_content = serde_json::to_string_pretty(cleaned_hooks)?;
            return Self::write_with_diff(&path, old_content, &new_content, dry_run);
        }
        if !dry_run {
            fs::remove_file(&path)?;
        }
        Ok(generate_diff(&path, old_content, ""))
    }
}

// --- HookInstaller implementation ---

impl HookInstaller for CodexInstaller {
    fn name(&self) -> &str {
        "Codex"
    }

    fn id(&self) -> &str {
        "codex"
    }

    fn process_names(&self) -> Vec<&str> {
        vec!["codex"]
    }

    fn check_hooks(&self, params: &HookInstallerParams) -> Result<HookCheckResult, GitAiError> {
        let has_binary = binary_exists("codex");
        let has_dotfiles = codex_home_dir().exists();

        if !has_binary && !has_dotfiles {
            return Ok(HookCheckResult::tool_not_installed());
        }

        let config_path = Self::config_path();
        let config = if config_path.exists() {
            Self::parse_config_toml(&fs::read_to_string(&config_path)?)?
        } else {
            TomlValue::Table(Map::new())
        };
        let hooks_json_path = Self::hooks_json_path();
        let hooks_json = if hooks_json_path.exists() {
            Self::parse_hooks_json(&fs::read_to_string(&hooks_json_path)?)?
        } else {
            json!({})
        };

        if Config::fresh().codex_hooks_format() == CodexHooksFormat::HooksJson {
            let (desired_config, desired_hooks_json) =
                Self::hooks_json_format_desired_state(&config, &hooks_json, &params.binary_path)?;
            let has_json_hooks = Self::hooks_json_has_git_ai_entries(&hooks_json);
            let hooks_installed = Self::config_hooks_feature_enabled(&config) && has_json_hooks;
            let hooks_up_to_date = config == desired_config && hooks_json == desired_hooks_json;

            return Ok(HookCheckResult::installed(
                hooks_installed,
                hooks_up_to_date,
            ));
        }

        let desired_config = Self::config_with_installed_hooks(&config, &params.binary_path)?;
        let has_inline_hooks = Self::config_has_inline_hooks(&config);
        let has_legacy_hooks_json = Self::hooks_json_has_git_ai_entries(&hooks_json);
        let hooks_installed = Self::config_hooks_feature_enabled(&config)
            && (has_inline_hooks || has_legacy_hooks_json);
        let hooks_up_to_date = config == desired_config && !has_legacy_hooks_json;

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
        let config_path = Self::config_path();
        let hooks_json_path = Self::hooks_json_path();

        let existing_config_content = if config_path.exists() {
            fs::read_to_string(&config_path)?
        } else {
            String::new()
        };

        let existing_config = Self::parse_config_toml(&existing_config_content)?;

        let existing_hooks_content = if hooks_json_path.exists() {
            fs::read_to_string(&hooks_json_path)?
        } else {
            String::new()
        };
        let existing_hooks = Self::parse_hooks_json(&existing_hooks_content)?;

        let mut diff_output = Vec::new();

        if Config::fresh().codex_hooks_format() == CodexHooksFormat::HooksJson {
            let (merged_config, merged_hooks) = Self::hooks_json_format_desired_state(
                &existing_config,
                &existing_hooks,
                &params.binary_path,
            )?;

            if existing_config != merged_config {
                diff_output.push(Self::write_config(
                    &merged_config,
                    &existing_config_content,
                    dry_run,
                )?);
            }
            if existing_hooks != merged_hooks {
                diff_output.push(Self::write_with_diff(
                    &hooks_json_path,
                    &existing_hooks_content,
                    &serde_json::to_string_pretty(&merged_hooks)?,
                    dry_run,
                )?);
            }
        } else {
            let merged_config =
                Self::config_with_installed_hooks(&existing_config, &params.binary_path)?;
            // A legacy hooks.json still holding git-ai entries needs migration.
            let (cleaned_hooks, hooks_json_changed) =
                Self::remove_codex_hooks_from_json(&existing_hooks)?;

            // Write config.toml FIRST (contains the replacement inline hooks)
            if existing_config != merged_config {
                diff_output.push(Self::write_config(
                    &merged_config,
                    &existing_config_content,
                    dry_run,
                )?);
            }
            // THEN clean up legacy hooks.json (safe: config.toml already has the hooks)
            if hooks_json_changed {
                diff_output.push(Self::write_cleaned_hooks_json(
                    &cleaned_hooks,
                    &existing_hooks_content,
                    dry_run,
                )?);
            }
        }

        Ok((!diff_output.is_empty()).then(|| diff_output.join("\n")))
    }

    fn uninstall_hooks(
        &self,
        _params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        let config_path = Self::config_path();
        let hooks_json_path = Self::hooks_json_path();
        if !config_path.exists() && !hooks_json_path.exists() {
            return Ok(None);
        }

        let existing_config_content = if config_path.exists() {
            fs::read_to_string(&config_path)?
        } else {
            String::new()
        };
        let existing_config = Self::parse_config_toml(&existing_config_content)?;

        // Remove inline hooks from config.toml
        let config_without_notify =
            Self::remove_notify_if_git_ai(&existing_config)?.unwrap_or(existing_config.clone());
        let (config_without_hooks, inline_hooks_changed) =
            Self::remove_inline_hooks_from_config(&config_without_notify)?;
        let merged_config = Self::remove_feature_flags(&config_without_hooks)?;

        // Check if legacy hooks.json needs cleanup
        let existing_hooks_content = if hooks_json_path.exists() {
            fs::read_to_string(&hooks_json_path)?
        } else {
            String::new()
        };
        let (cleaned_hooks, hooks_json_changed) =
            Self::remove_codex_hooks_from_json(&Self::parse_hooks_json(&existing_hooks_content)?)?;

        let mut diff_output = Vec::new();

        // Write config.toml changes first
        if merged_config != existing_config || inline_hooks_changed {
            diff_output.push(Self::write_config(
                &merged_config,
                &existing_config_content,
                dry_run,
            )?);
        }
        // Then clean up legacy hooks.json
        if hooks_json_changed {
            diff_output.push(Self::write_cleaned_hooks_json(
                &cleaned_hooks,
                &existing_hooks_content,
                dry_run,
            )?);
        }

        Ok((!diff_output.is_empty()).then(|| diff_output.join("\n")))
    }
}
