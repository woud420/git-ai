use crate::error::GitAiError;
use crate::operations::mdm::editor_cli::binary_exists;
use crate::operations::mdm::hook_installer::{HookCheckResult, HookInstaller, HookInstallerParams};
use crate::operations::mdm::hooks_merge::{
    catch_all_hook_status, check_settings_hooks, install_catch_all_hooks_at, parse_json,
    uninstall_catch_all_hooks_at,
};
use crate::operations::mdm::paths::{claude_config_dir, normalize_windows_path_for_shell};
use crate::operations::mdm::version::{
    MIN_CLAUDE_VERSION, get_binary_version, parse_version, version_meets_requirement,
};
use serde_json::Value;
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

        install_catch_all_hooks_at(
            settings_path,
            dry_run,
            parse_json,
            &[
                ("PreToolUse", pre_tool_cmd.as_str()),
                ("PostToolUse", post_tool_cmd.as_str()),
            ],
            |_settings| {},
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
            &["PreToolUse", "PostToolUse"],
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

        check_settings_hooks(&Self::settings_path(), parse_json, Self::hook_status)
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
mod tests {
    use super::*;
    use crate::operations::mdm::agents::catch_all_scenarios::{
        CatchAllAgent, binary_path, block, block_hooks, blocks, catch_all_scenario_tests,
        has_command, has_git_ai, hooks_in_catch_all, matcher_hooks, params, read_settings,
        setup_test_env, write_settings,
    };
    use crate::operations::mdm::paths::clean_path;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    const AGENT: CatchAllAgent = CatchAllAgent {
        install_at: ClaudeCodeInstaller::install_hooks_at,
        uninstall_at: ClaudeCodeInstaller::uninstall_hooks_at,
        hook_status: ClaudeCodeInstaller::hook_status,
        hooks: [
            ("PreToolUse", CLAUDE_PRE_TOOL_CMD),
            ("PostToolUse", CLAUDE_POST_TOOL_CMD),
        ],
        old_matcher: "Write|Edit|MultiEdit",
        stale_cmd: "/old/path/git-ai checkpoint claude --hook-input stdin",
        add_install_settings: |_| {},
    };

    catch_all_scenario_tests! { AGENT;
        s1_fresh_install_creates_catch_all_block,
        s2_idempotent_already_on_catch_all,
        s3_migration_old_matcher_no_user_hooks,
        s4_migration_old_matcher_user_hook_preserved,
        s5_fresh_install_user_has_old_matcher_hook,
        s6_fresh_install_user_has_catch_all_hook,
        s7_idempotent_user_catch_all_plus_git_ai,
        s8_deduplication_git_ai_in_both_catch_all_and_old_matcher,
        s9_deduplication_two_git_ai_in_catch_all_block,
        s10_stale_command_upgraded_in_catch_all,
        u1_uninstall_from_catch_all,
        u2_uninstall_from_old_matcher_preserves_user_hook,
        u3_uninstall_from_multiple_blocks_preserves_user_hooks,
        u4_noop_uninstall_when_no_git_ai,
        c1_no_hooks_returns_not_installed,
        c2_git_ai_in_catch_all_returns_up_to_date,
        c3_git_ai_only_in_old_matcher_returns_installed_but_not_up_to_date,
    }

    fn expected_cmd() -> String {
        format!("{} {}", binary_path().display(), CLAUDE_PRE_TOOL_CMD)
    }

    /// Writes the same `blocks` under both PreToolUse and PostToolUse.
    fn write_both_hook_types(path: &Path, blocks: Value) {
        write_settings(
            path,
            &json!({"hooks": {"PreToolUse": blocks, "PostToolUse": blocks}}),
        );
    }

    #[test]
    fn s11_git_ai_in_arbitrary_old_matcher_migrated() {
        let (_td, path) = setup_test_env();
        let cmd = expected_cmd();
        write_both_hook_types(&path, json!([block("Bash", &["user-bash-hook", &cmd])]));

        ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

        let settings = read_settings(&path);
        for hook_type in ["PreToolUse", "PostToolUse"] {
            // git-ai now in catch-all
            assert_eq!(hooks_in_catch_all(&settings, hook_type).len(), 1);

            // user-bash-hook preserved in Bash block, git-ai removed
            let bash_hooks = matcher_hooks(&settings, hook_type, "Bash");
            assert!(has_command(bash_hooks, "user-bash-hook"));
            assert!(!has_git_ai(bash_hooks));
        }
    }

    #[test]
    fn s12_git_ai_spread_across_multiple_old_blocks() {
        let (_td, path) = setup_test_env();
        let cmd = expected_cmd();
        write_both_hook_types(
            &path,
            json!([
                block("Write|Edit|MultiEdit", &[&cmd]),
                block("Bash", &[&cmd]),
            ]),
        );

        ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

        let settings = read_settings(&path);
        for hook_type in ["PreToolUse", "PostToolUse"] {
            // exactly one git-ai total, in catch-all
            let git_ai_blocks: Vec<&Value> = blocks(&settings, hook_type)
                .iter()
                .filter(|b| has_git_ai(block_hooks(b)))
                .collect();
            assert_eq!(git_ai_blocks.len(), 1);
            assert_eq!(git_ai_blocks[0]["matcher"], CLAUDE_CATCH_ALL_MATCHER);
        }
    }

    #[test]
    fn s13_hook_types_handled_independently() {
        let (_td, path) = setup_test_env();
        let cmd = expected_cmd();
        // PreToolUse: git-ai on old matcher; PostToolUse: git-ai already on catch-all
        write_settings(
            &path,
            &json!({
                "hooks": {
                    "PreToolUse": [block("Write|Edit|MultiEdit", &[&cmd])],
                    "PostToolUse": [block("*", &[&cmd])]
                }
            }),
        );

        ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

        let settings = read_settings(&path);

        // PreToolUse: migrated to catch-all
        let pre_catch = hooks_in_catch_all(&settings, "PreToolUse");
        assert_eq!(pre_catch.len(), 1);

        // PostToolUse: unchanged, still exactly one in catch-all
        let post_catch = hooks_in_catch_all(&settings, "PostToolUse");
        assert_eq!(post_catch.len(), 1);
    }

    // ---- Path / Windows tests (preserved from original) ----

    #[test]
    fn test_claude_hook_commands_no_windows_extended_path_prefix() {
        let raw_path = PathBuf::from(r"\\?\C:\Users\USERNAME\.git-ai\bin\git-ai.exe");
        let binary_path = clean_path(raw_path);

        let binary_path_str = normalize_windows_path_for_shell(&binary_path);
        let pre_tool_cmd = format!("{} {}", binary_path_str, CLAUDE_PRE_TOOL_CMD);
        let post_tool_cmd = format!("{} {}", binary_path_str, CLAUDE_POST_TOOL_CMD);

        assert!(
            !pre_tool_cmd.contains(r"\\?\"),
            "PreToolUse command should not contain \\\\?\\ prefix, got: {}",
            pre_tool_cmd
        );
        assert!(
            !post_tool_cmd.contains(r"\\?\"),
            "PostToolUse command should not contain \\\\?\\ prefix, got: {}",
            post_tool_cmd
        );
        assert!(
            pre_tool_cmd.contains("checkpoint claude"),
            "command should still contain checkpoint args"
        );
    }

    #[test]
    fn test_claude_hook_commands_use_forward_slash_path_on_windows() {
        let binary_path = PathBuf::from(r"C:\Users\Administrator\.git-ai\bin\git-ai.exe");
        let binary_path_str = normalize_windows_path_for_shell(&binary_path);
        let pre_tool_cmd = format!("{} {}", binary_path_str, CLAUDE_PRE_TOOL_CMD);
        let post_tool_cmd = format!("{} {}", binary_path_str, CLAUDE_POST_TOOL_CMD);

        assert_eq!(
            pre_tool_cmd,
            "C:/Users/Administrator/.git-ai/bin/git-ai.exe checkpoint claude --hook-input stdin",
            "PreToolUse command should use forward-slash path format"
        );
        assert_eq!(
            post_tool_cmd,
            "C:/Users/Administrator/.git-ai/bin/git-ai.exe checkpoint claude --hook-input stdin",
            "PostToolUse command should use forward-slash path format"
        );
    }

    #[test]
    fn test_claude_hook_commands_preserve_unix_path() {
        let binary_path = PathBuf::from("/usr/local/bin/git-ai");
        let binary_path_str = normalize_windows_path_for_shell(&binary_path);
        let pre_tool_cmd = format!("{} {}", binary_path_str, CLAUDE_PRE_TOOL_CMD);

        assert_eq!(
            pre_tool_cmd, "/usr/local/bin/git-ai checkpoint claude --hook-input stdin",
            "Unix paths should be preserved unchanged"
        );
    }

    /// install_hooks_at should succeed even when
    /// the parent directory does not yet exist.
    #[test]
    fn test_install_hooks_creates_missing_parent_dir() {
        let temp_dir = TempDir::new().unwrap();
        // Point to a settings.json inside a directory that does NOT exist yet
        let settings_path = temp_dir.path().join("missing_dir").join("settings.json");
        assert!(!settings_path.parent().unwrap().exists());

        let result =
            ClaudeCodeInstaller::install_hooks_at(&settings_path, &params(), false).unwrap();

        assert!(result.is_some(), "should report changes for fresh install");
        assert!(settings_path.exists(), "settings.json should be created");

        let content: Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).expect("valid JSON");
        let hooks = content.get("hooks").expect("hooks key should exist");
        assert!(hooks.get("PreToolUse").is_some());
        assert!(hooks.get("PostToolUse").is_some());
    }
}
