use crate::error::GitAiError;
use crate::operations::mdm::editor_cli::binary_exists;
use crate::operations::mdm::hook_installer::{HookCheckResult, HookInstaller, HookInstallerParams};
use crate::operations::mdm::hooks_merge::{
    catch_all_hook_status, check_settings_hooks, install_catch_all_hooks_at, parse_json,
    uninstall_catch_all_hooks_at,
};
use crate::operations::mdm::paths::gemini_config_dir;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

// Command patterns for hooks
const GEMINI_BEFORE_TOOL_CMD: &str = "checkpoint gemini --hook-input stdin";
const GEMINI_AFTER_TOOL_CMD: &str = "checkpoint gemini --hook-input stdin";

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

        install_catch_all_hooks_at(
            settings_path,
            dry_run,
            parse_json,
            &[
                ("BeforeTool", before_tool_cmd.as_str()),
                ("AfterTool", after_tool_cmd.as_str()),
            ],
            |merged| {
                // Ensure tools.enableHooks is set to true.
                if let Some(tools_obj) = merged.get_mut("tools").and_then(|t| t.as_object_mut()) {
                    if tools_obj.get("enableHooks") != Some(&json!(true)) {
                        tools_obj.insert("enableHooks".to_string(), json!(true));
                    }
                } else if let Some(root) = merged.as_object_mut() {
                    root.insert("tools".to_string(), json!({ "enableHooks": true }));
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
            parse_json,
            &["BeforeTool", "AfterTool"],
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

        check_settings_hooks(&Self::settings_path(), parse_json, Self::hook_status)
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
        CatchAllAgent, binary_path, catch_all_scenario_tests, hooks_in_catch_all, params,
        read_settings, setup_test_env,
    };
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

    const AGENT: CatchAllAgent = CatchAllAgent {
        install_at: GeminiInstaller::install_hooks_at,
        uninstall_at: GeminiInstaller::uninstall_hooks_at,
        hook_status: GeminiInstaller::hook_status,
        hooks: [
            ("BeforeTool", GEMINI_BEFORE_TOOL_CMD),
            ("AfterTool", GEMINI_AFTER_TOOL_CMD),
        ],
        old_matcher: "write_file|replace",
        stale_cmd: "/old/git-ai checkpoint gemini",
        add_install_settings: |settings| settings["tools"]["enableHooks"] = json!(true),
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

    fn with_temp_home<F: FnOnce(&Path)>(f: F) {
        let temp_dir = TempDir::new().unwrap();
        let home = temp_dir.path().to_path_buf();

        let prev_home = std::env::var_os("HOME");
        let prev_userprofile = std::env::var_os("USERPROFILE");
        let prev_gemini_cli_home = std::env::var_os("GEMINI_CLI_HOME");

        // SAFETY: tests are serialized via #[serial], so mutating process env is safe.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("USERPROFILE", &home);
            std::env::remove_var("GEMINI_CLI_HOME");
        }

        f(&home);

        // SAFETY: tests are serialized via #[serial], so restoring process env is safe.
        unsafe {
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match prev_userprofile {
                Some(v) => std::env::set_var("USERPROFILE", v),
                None => std::env::remove_var("USERPROFILE"),
            }
            match prev_gemini_cli_home {
                Some(v) => std::env::set_var("GEMINI_CLI_HOME", v),
                None => std::env::remove_var("GEMINI_CLI_HOME"),
            }
        }
    }

    fn expected_before_cmd() -> String {
        format!("{} {}", binary_path().display(), GEMINI_BEFORE_TOOL_CMD)
    }

    fn expected_after_cmd() -> String {
        format!("{} {}", binary_path().display(), GEMINI_AFTER_TOOL_CMD)
    }

    #[test]
    #[serial]
    fn test_install_hooks_respects_gemini_cli_home() {
        with_temp_home(|home| {
            let default_gemini_dir = home.join(".gemini");
            let custom_gemini_home = home.join("custom-gemini-home");
            fs::create_dir_all(&custom_gemini_home).unwrap();

            // SAFETY: tests are serialized via #[serial], so mutating process env is safe.
            unsafe {
                std::env::set_var("GEMINI_CLI_HOME", &custom_gemini_home);
            }

            let settings_path = custom_gemini_home.join(".gemini").join("settings.json");
            fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
            fs::write(&settings_path, "{}").unwrap();

            let installer = GeminiInstaller;
            let result = installer.install_hooks(&params(), false).unwrap();
            assert!(result.is_some(), "install should report a settings diff");
            assert!(
                !default_gemini_dir.exists(),
                "default ~/.gemini should not be touched when GEMINI_CLI_HOME is set"
            );

            let settings = read_settings(&settings_path);
            assert_eq!(settings["tools"]["enableHooks"], true);
            assert_eq!(
                hooks_in_catch_all(&settings, "BeforeTool")[0]["command"],
                expected_before_cmd()
            );
            assert_eq!(
                hooks_in_catch_all(&settings, "AfterTool")[0]["command"],
                expected_after_cmd()
            );

            let check = installer.check_hooks(&params()).unwrap();
            assert!(check.tool_installed);
            assert!(check.hooks_installed);
            assert!(check.hooks_up_to_date);
        });
    }

    #[test]
    fn s11_enables_hooks_when_missing() {
        let (_td, path) = setup_test_env();
        // No tools.enableHooks set
        fs::write(&path, "{}").unwrap();

        GeminiInstaller::install_hooks_at(&path, &params(), false).unwrap();

        let settings = read_settings(&path);
        assert_eq!(
            settings.get("tools").and_then(|t| t.get("enableHooks")),
            Some(&json!(true))
        );
    }
}
