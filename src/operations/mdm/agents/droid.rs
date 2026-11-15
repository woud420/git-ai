use crate::error::GitAiError;
use crate::operations::mdm::editor_cli::binary_exists;
use crate::operations::mdm::hook_installer::{HookCheckResult, HookInstaller, HookInstallerParams};
#[cfg(test)]
use crate::operations::mdm::hooks_merge::is_git_ai_checkpoint_command;
use crate::operations::mdm::hooks_merge::{
    catch_all_hook_status, check_settings_hooks, install_catch_all_hooks_at,
    uninstall_catch_all_hooks_at,
};
use crate::operations::mdm::paths::home_dir;
use jsonc_parser::ParseOptions;
use serde_json::{Value, json};
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

        install_catch_all_hooks_at(
            settings_path,
            dry_run,
            parse_jsonc_settings,
            &[
                ("PreToolUse", pre_tool_cmd.as_str()),
                ("PostToolUse", post_tool_cmd.as_str()),
            ],
            |merged| {
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
        uninstall_catch_all_hooks_at(
            settings_path,
            dry_run,
            parse_jsonc_settings,
            &["PreToolUse", "PostToolUse"],
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

        check_settings_hooks(
            &Self::settings_path(),
            parse_jsonc_settings,
            Self::hook_status,
        )
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
mod tests {
    use super::*;
    use crate::operations::mdm::agents::catch_all_scenarios::{
        CatchAllAgent, binary_path, catch_all_scenario_tests, command, hooks_in_catch_all, params,
        read_settings, setup_test_env,
    };
    use crate::operations::mdm::test_env::{with_fake_binary_on_path, with_temp_home};
    use serial_test::serial;
    use std::fs;

    const AGENT: CatchAllAgent = CatchAllAgent {
        install_at: DroidInstaller::install_hooks_at,
        uninstall_at: DroidInstaller::uninstall_hooks_at,
        hook_status: DroidInstaller::hook_status,
        hooks: [
            ("PreToolUse", DROID_PRE_TOOL_CMD),
            ("PostToolUse", DROID_POST_TOOL_CMD),
        ],
        old_matcher: "^(Edit|Write|Create|ApplyPatch)$",
        stale_cmd: "/old/git-ai checkpoint droid",
        add_install_settings: |settings| settings["hooks"]["claudeHooksImported"] = json!(true),
    };

    catch_all_scenario_tests! { AGENT;
        s1_fresh_install_creates_catch_all_block,
        s2_idempotent_already_on_catch_all,
        s3_migration_old_matcher_no_user_hooks,
        s4_migration_old_matcher_user_hook_preserved,
        s5_fresh_install_user_has_old_matcher_hook,
        s6_fresh_install_user_has_catch_all_hook,
        s7_idempotent_user_catch_all_plus_git_ai,
        s8_deduplication_git_ai_in_both_blocks
            => s8_deduplication_git_ai_in_both_catch_all_and_old_matcher,
        s9_deduplication_two_git_ai_in_catch_all => s9_deduplication_two_git_ai_in_catch_all_block,
        s10_stale_command_upgraded => s10_stale_command_upgraded_in_catch_all,
        u1_uninstall_from_catch_all,
        u2_uninstall_from_old_matcher_preserves_user_hook,
        u3_uninstall_from_multiple_blocks => u3_uninstall_from_multiple_blocks_preserves_user_hooks,
        u4_noop_uninstall_when_no_git_ai,
        c1_no_hooks_returns_not_installed,
        c2_git_ai_in_catch_all_returns_up_to_date,
        c3_git_ai_only_in_old_matcher_not_up_to_date
            => c3_git_ai_only_in_old_matcher_returns_installed_but_not_up_to_date,
    }

    fn expected_cmd() -> String {
        format!("{} {}", binary_path().display(), DROID_PRE_TOOL_CMD)
    }

    // ---- JSONC settings ----

    #[test]
    fn s11_install_into_jsonc_settings_with_comments() {
        let (_td, path) = setup_test_env();
        let jsonc_content = r#"// Factory CLI Settings
// This file contains your Factory CLI configuration.
{
  "model": "claude-opus-4-5-20251101",
  // Some inline comment
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {"type": "command", "command": "echo existing"}
        ]
      }
    ]
  }
}"#;
        fs::write(&path, jsonc_content).unwrap();

        let diff = DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();
        assert!(diff.is_some());

        let settings = read_settings(&path);
        let catch_all = hooks_in_catch_all(&settings, "PreToolUse");
        assert_eq!(catch_all.len(), 2);
        assert_eq!(command(&catch_all[0]), "echo existing");
        assert!(is_git_ai_checkpoint_command(command(&catch_all[1])));
    }

    #[test]
    fn s12_install_into_jsonc_settings_with_trailing_commas() {
        let (_td, path) = setup_test_env();
        let jsonc_content = r#"{
  "allowlist": ["a", "b",],
  "hooks": {},
}"#;
        fs::write(&path, jsonc_content).unwrap();

        let diff = DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();
        assert!(diff.is_some());

        let settings = read_settings(&path);
        let catch_all = hooks_in_catch_all(&settings, "PreToolUse");
        assert_eq!(catch_all.len(), 1);
    }

    #[test]
    fn u5_uninstall_from_jsonc_settings_with_comments() {
        let (_td, path) = setup_test_env();
        let cmd = expected_cmd();
        let jsonc_content = format!(
            r#"// Factory CLI Settings
{{
  "hooks": {{
    "PreToolUse": [{{"matcher": "*", "hooks": [{{"type":"command","command": "{cmd}"}}]}}],
    "PostToolUse": [{{"matcher": "*", "hooks": [{{"type":"command","command": "{cmd}"}}]}}],
    "claudeHooksImported": true
  }}
}}"#
        );
        fs::write(&path, jsonc_content).unwrap();

        let diff = DroidInstaller::uninstall_hooks_at(&path, false).unwrap();
        assert!(diff.is_some());
    }

    // ---- JSONC parsing ----

    #[test]
    fn jsonc_parse_with_line_comments() {
        let input = r#"// header comment
{
  "key": "value", // inline comment
  "num": 42
}"#;
        let val = parse_jsonc_settings(input).unwrap();
        assert_eq!(val.get("key").and_then(|v| v.as_str()), Some("value"));
        assert_eq!(val.get("num").and_then(|v| v.as_i64()), Some(42));
    }

    #[test]
    fn jsonc_parse_with_block_comments() {
        let input = r#"{ /* block */ "key": "value" }"#;
        let val = parse_jsonc_settings(input).unwrap();
        assert_eq!(val.get("key").and_then(|v| v.as_str()), Some("value"));
    }

    #[test]
    fn jsonc_parse_with_trailing_commas() {
        let input = r#"{ "a": [1, 2,], "b": 3, }"#;
        let val = parse_jsonc_settings(input).unwrap();
        assert_eq!(val.get("b").and_then(|v| v.as_i64()), Some(3));
    }

    #[test]
    fn jsonc_parse_empty_returns_empty_object() {
        let val = parse_jsonc_settings("").unwrap();
        assert_eq!(val, json!({}));
    }

    // ---- Detection / check_hooks ----

    #[test]
    #[serial]
    fn c4_binary_on_path_without_dotfiles_detects_tool() {
        with_temp_home(|_home| {
            with_fake_binary_on_path("droid", |_| {
                let installer = DroidInstaller;
                let result = installer.check_hooks(&params()).unwrap();
                assert!(
                    result.tool_installed,
                    "droid binary on PATH should be detected even without ~/.factory"
                );
                assert!(!result.hooks_installed);
                assert!(!result.hooks_up_to_date);
            });
        });
    }

    #[test]
    #[serial]
    fn c5_no_binary_no_dotfiles_not_detected() {
        with_temp_home(|_home| {
            let installer = DroidInstaller;
            let result = installer.check_hooks(&params()).unwrap();
            assert!(
                !result.tool_installed,
                "no binary and no ~/.factory should mean tool_installed=false"
            );
        });
    }

    #[test]
    #[serial]
    fn c6_dotfiles_without_binary_detects_tool() {
        with_temp_home(|home| {
            fs::create_dir_all(home.join(".factory")).unwrap();
            let installer = DroidInstaller;
            let result = installer.check_hooks(&params()).unwrap();
            assert!(
                result.tool_installed,
                "~/.factory dir should be enough to detect tool even without binary"
            );
        });
    }

    #[test]
    #[serial]
    fn c7_binary_on_path_install_creates_settings() {
        with_temp_home(|_home| {
            with_fake_binary_on_path("droid", |_| {
                let installer = DroidInstaller;
                let result = installer.install_hooks(&params(), false).unwrap();
                assert!(result.is_some(), "install_hooks should produce a diff");

                let settings_path = DroidInstaller::settings_path();
                assert!(
                    settings_path.exists(),
                    "install_hooks should create ~/.factory/settings.json"
                );

                let content = fs::read_to_string(&settings_path).unwrap();
                assert!(
                    content.contains("checkpoint droid"),
                    "settings.json should contain the checkpoint hook command"
                );
            });
        });
    }
}
