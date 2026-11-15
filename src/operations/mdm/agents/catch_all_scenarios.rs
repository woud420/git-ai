//! Install, uninstall and check scenarios shared by the installers built on the
//! catch-all matcher helpers in `hooks_merge` (Claude Code, Droid, Gemini).
//! Each scenario takes the agent's [`CatchAllAgent`]; an agent's test module
//! turns them into `#[test]`s with [`catch_all_scenario_tests!`].

use crate::error::GitAiError;
use crate::operations::mdm::hook_installer::HookInstallerParams;
use crate::operations::mdm::hooks_merge::is_git_ai_checkpoint_command;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Expands each `test_name [=> scenario]` entry to a `#[test]` that runs the
/// scenario of that name (default: `test_name`) against `$agent`.
macro_rules! catch_all_scenario_tests {
    (@test $agent:expr, $test:ident) => {
        catch_all_scenario_tests!(@test $agent, $test, $test);
    };
    (@test $agent:expr, $test:ident, $scenario:ident) => {
        #[test]
        fn $test() {
            crate::operations::mdm::agents::catch_all_scenarios::$scenario(&$agent);
        }
    };
    ($agent:expr; $($test:ident $(=> $scenario:ident)?),+ $(,)?) => {
        $(catch_all_scenario_tests!(@test $agent, $test $(, $scenario)?);)+
    };
}
pub(super) use catch_all_scenario_tests;

type InstallAt = fn(&Path, &HookInstallerParams, bool) -> Result<Option<String>, GitAiError>;
type UninstallAt = fn(&Path, bool) -> Result<Option<String>, GitAiError>;

/// What differs between the catch-all matcher installers.
pub struct CatchAllAgent {
    pub install_at: InstallAt,
    pub uninstall_at: UninstallAt,
    pub hook_status: fn(&Value) -> (bool, bool),
    /// `(hook type, checkpoint arguments)` of the before-edit and after-edit hooks.
    pub hooks: [(&'static str, &'static str); 2],
    /// The matcher older git-ai versions installed under, before `"*"`.
    pub old_matcher: &'static str,
    /// A git-ai command left by an older install at another path.
    pub stale_cmd: &'static str,
    /// Adds the non-hook settings an install writes (none for Claude Code).
    pub add_install_settings: fn(&mut Value),
}

impl CatchAllAgent {
    /// `(hook type, git-ai command the installer writes for it)`.
    pub fn hook_cmds(&self) -> [(&'static str, String); 2] {
        self.hooks
            .map(|(hook_type, args)| (hook_type, format!("{} {}", binary_path().display(), args)))
    }

    /// The hook type `hook_status` inspects.
    fn first_hook_type(&self) -> &'static str {
        self.hooks[0].0
    }

    fn install(&self, path: &Path) -> Option<String> {
        (self.install_at)(path, &params(), false).unwrap()
    }

    fn uninstall(&self, path: &Path) -> Option<String> {
        (self.uninstall_at)(path, false).unwrap()
    }

    /// Settings as an install leaves them, with each hook type's blocks built
    /// by `blocks(index, git-ai command)`; index 0 is the before-edit hook.
    fn installed_settings(&self, blocks: impl Fn(usize, &str) -> Value) -> Value {
        let mut settings = json!({"hooks": {}});
        for (i, (hook_type, cmd)) in self.hook_cmds().iter().enumerate() {
            settings["hooks"][*hook_type] = blocks(i, cmd);
        }
        (self.add_install_settings)(&mut settings);
        settings
    }
}

pub fn setup_test_env() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let settings_path = temp_dir.path().join("config").join("settings.json");
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    (temp_dir, settings_path)
}

pub fn binary_path() -> PathBuf {
    PathBuf::from("/usr/local/bin/git-ai")
}

pub fn params() -> HookInstallerParams {
    HookInstallerParams {
        binary_path: binary_path(),
    }
}

pub fn read_settings(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

pub fn write_settings(path: &Path, settings: &Value) {
    fs::write(path, serde_json::to_string_pretty(settings).unwrap()).unwrap();
}

/// A matcher block running `cmds` as command hooks.
pub fn block(matcher: &str, cmds: &[&str]) -> Value {
    let hooks: Vec<Value> = cmds
        .iter()
        .map(|cmd| json!({"type": "command", "command": cmd}))
        .collect();
    json!({"matcher": matcher, "hooks": hooks})
}

/// `{"hooks": {<hook_type>: blocks}}`.
fn hooks_for(hook_type: &str, blocks: Value) -> Value {
    let mut settings = json!({"hooks": {}});
    settings["hooks"][hook_type] = blocks;
    settings
}

pub fn command(hook: &Value) -> &str {
    hook["command"].as_str().unwrap()
}

pub fn has_command(hooks: &[Value], cmd: &str) -> bool {
    hooks.iter().any(|h| h["command"] == cmd)
}

pub fn has_git_ai(hooks: &[Value]) -> bool {
    hooks.iter().any(|h| {
        h["command"]
            .as_str()
            .is_some_and(is_git_ai_checkpoint_command)
    })
}

/// The hooks of a matcher block (empty when it has none).
pub fn block_hooks(block: &Value) -> &[Value] {
    block["hooks"].as_array().map_or(&[], Vec::as_slice)
}

/// Every matcher block of `hook_type`.
pub fn blocks<'a>(settings: &'a Value, hook_type: &str) -> &'a [Value] {
    settings["hooks"][hook_type]
        .as_array()
        .unwrap_or_else(|| panic!("{hook_type}: no hook blocks"))
}

/// The hooks of the `matcher` block of `hook_type`.
pub fn matcher_hooks<'a>(settings: &'a Value, hook_type: &str, matcher: &str) -> &'a [Value] {
    let block = blocks(settings, hook_type)
        .iter()
        .find(|b| b["matcher"] == matcher)
        .unwrap_or_else(|| panic!("{hook_type}: `{matcher}` matcher block should still exist"));
    block_hooks(block)
}

/// The hooks of the `"*"` catch-all block of `hook_type` (empty when absent).
pub fn hooks_in_catch_all<'a>(settings: &'a Value, hook_type: &str) -> &'a [Value] {
    settings["hooks"][hook_type]
        .as_array()
        .and_then(|blocks| blocks.iter().find(|b| b["matcher"] == "*"))
        .map_or(&[], block_hooks)
}

// ---- Install scenarios ----

pub fn s1_fresh_install_creates_catch_all_block(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    // File does not exist yet
    fs::remove_file(&path).ok();

    assert!(agent.install(&path).is_some(), "should produce a diff");

    let settings = read_settings(&path);
    for (hook_type, cmd) in agent.hook_cmds() {
        let hooks = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(hooks.len(), 1, "{hook_type}: expected 1 hook in catch-all");
        assert_eq!(command(&hooks[0]), cmd);
    }
    // The install also wrote the agent's own settings: adding them again is a no-op.
    let mut with_install_settings = settings.clone();
    (agent.add_install_settings)(&mut with_install_settings);
    assert_eq!(settings, with_install_settings);
}

pub fn s2_idempotent_already_on_catch_all(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    write_settings(
        &path,
        &agent.installed_settings(|_, cmd| json!([block("*", &[cmd])])),
    );

    let diff = agent.install(&path);
    assert!(diff.is_none(), "should return None when already up-to-date");
}

pub fn s3_migration_old_matcher_no_user_hooks(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    write_settings(
        &path,
        &agent.installed_settings(|_, cmd| json!([block(agent.old_matcher, &[cmd])])),
    );

    agent.install(&path);

    let settings = read_settings(&path);
    for (hook_type, _) in agent.hooks {
        let hooks = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(hooks.len(), 1, "{hook_type}: expected git-ai in catch-all");

        // The old matcher block had only our hook, so it must be removed entirely.
        assert_eq!(
            blocks(&settings, hook_type).len(),
            1,
            "{hook_type}: old matcher block should be removed, only catch-all should remain"
        );
    }
}

pub fn s4_migration_old_matcher_user_hook_preserved(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    let user_cmds = ["echo before", "prettier --write"];
    write_settings(
        &path,
        &agent.installed_settings(|i, cmd| json!([block(agent.old_matcher, &[user_cmds[i], cmd])])),
    );

    agent.install(&path);

    let settings = read_settings(&path);
    for ((hook_type, _), user_cmd) in agent.hooks.into_iter().zip(user_cmds) {
        // git-ai in catch-all
        assert_eq!(hooks_in_catch_all(&settings, hook_type).len(), 1);

        // user hook still in old matcher block, git-ai not
        let old_hooks = matcher_hooks(&settings, hook_type, agent.old_matcher);
        assert!(
            has_command(old_hooks, user_cmd),
            "{hook_type}: user hook '{user_cmd}' should still be in old matcher block"
        );
        assert!(
            !has_git_ai(old_hooks),
            "{hook_type}: git-ai should not be in old matcher block after migration"
        );
    }
}

pub fn s5_fresh_install_user_has_old_matcher_hook(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    let user_cmd = "prettier --write";
    write_settings(
        &path,
        &agent.installed_settings(|_, _| json!([block(agent.old_matcher, &[user_cmd])])),
    );

    agent.install(&path);

    let settings = read_settings(&path);
    for (hook_type, _) in agent.hooks {
        // git-ai in new catch-all block
        assert_eq!(hooks_in_catch_all(&settings, hook_type).len(), 1);

        // user hook untouched in old block
        let old_hooks = matcher_hooks(&settings, hook_type, agent.old_matcher);
        assert_eq!(old_hooks.len(), 1);
        assert_eq!(command(&old_hooks[0]), user_cmd);
    }
}

pub fn s6_fresh_install_user_has_catch_all_hook(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    write_settings(
        &path,
        &agent.installed_settings(|_, _| json!([block("*", &["my-audit-tool"])])),
    );

    agent.install(&path);

    let settings = read_settings(&path);
    for (hook_type, cmd) in agent.hook_cmds() {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(
            catch_all.len(),
            2,
            "{hook_type}: should have user hook + git-ai"
        );
        assert_eq!(
            command(&catch_all[0]),
            "my-audit-tool",
            "user hook should be first"
        );
        assert!(is_git_ai_checkpoint_command(command(&catch_all[1])));
        assert_eq!(command(&catch_all[1]), cmd);
    }
}

pub fn s7_idempotent_user_catch_all_plus_git_ai(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    write_settings(
        &path,
        &agent.installed_settings(|_, cmd| json!([block("*", &["my-audit-tool", cmd])])),
    );

    let diff = agent.install(&path);
    assert!(diff.is_none(), "should be idempotent");
}

pub fn s8_deduplication_git_ai_in_both_catch_all_and_old_matcher(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    let user_cmd = "echo user-hook";
    write_settings(
        &path,
        &agent.installed_settings(|_, cmd| {
            json!([
                block("*", &[cmd]),
                block(agent.old_matcher, &[user_cmd, cmd]),
            ])
        }),
    );

    agent.install(&path);

    let settings = read_settings(&path);
    for (hook_type, _) in agent.hooks {
        // exactly one git-ai in catch-all
        assert_eq!(hooks_in_catch_all(&settings, hook_type).len(), 1);

        // old matcher block has user hook but NOT git-ai
        let old_hooks = matcher_hooks(&settings, hook_type, agent.old_matcher);
        assert!(has_command(old_hooks, user_cmd));
        assert!(!has_git_ai(old_hooks));
    }
}

pub fn s9_deduplication_two_git_ai_in_catch_all_block(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    write_settings(
        &path,
        &agent.installed_settings(|_, cmd| json!([block("*", &[cmd, cmd])])),
    );

    agent.install(&path);

    let settings = read_settings(&path);
    for (hook_type, _) in agent.hooks {
        assert_eq!(
            hooks_in_catch_all(&settings, hook_type).len(),
            1,
            "{hook_type}: should have exactly 1 after dedup"
        );
    }
}

pub fn s10_stale_command_upgraded_in_catch_all(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    write_settings(
        &path,
        &agent.installed_settings(|_, _| json!([block("*", &[agent.stale_cmd])])),
    );

    agent.install(&path);

    let settings = read_settings(&path);
    for (hook_type, cmd) in agent.hook_cmds() {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 1);
        assert_eq!(command(&catch_all[0]), cmd);
    }
}

// ---- Uninstall scenarios ----

pub fn u1_uninstall_from_catch_all(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    write_settings(
        &path,
        &agent.installed_settings(|_, cmd| json!([block("*", &[cmd])])),
    );

    assert!(agent.uninstall(&path).is_some());

    let settings = read_settings(&path);
    for (hook_type, _) in agent.hooks {
        assert!(
            !has_git_ai(hooks_in_catch_all(&settings, hook_type)),
            "{hook_type}: git-ai should be removed"
        );
    }
}

pub fn u2_uninstall_from_old_matcher_preserves_user_hook(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    write_settings(
        &path,
        &agent
            .installed_settings(|_, cmd| json!([block(agent.old_matcher, &["echo before", cmd])])),
    );

    agent.uninstall(&path);

    let settings = read_settings(&path);
    for (hook_type, _) in agent.hooks {
        let hooks = matcher_hooks(&settings, hook_type, agent.old_matcher);
        assert!(has_command(hooks, "echo before"));
        assert!(!has_git_ai(hooks));
    }
}

pub fn u3_uninstall_from_multiple_blocks_preserves_user_hooks(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    let user = "echo user";
    write_settings(
        &path,
        &agent.installed_settings(|i, cmd| match i {
            0 => json!([block("*", &[cmd, user]), block(agent.old_matcher, &[cmd])]),
            _ => json!([block("*", &[cmd]), block(agent.old_matcher, &[cmd, user])]),
        }),
    );

    agent.uninstall(&path);

    let settings = read_settings(&path);
    for (hook_type, _) in agent.hooks {
        let all_blocks = blocks(&settings, hook_type);
        // No git-ai anywhere
        for block in all_blocks {
            assert!(!has_git_ai(block_hooks(block)));
        }
        // user hook still present somewhere
        assert!(all_blocks.iter().any(|b| has_command(block_hooks(b), user)));
    }
}

pub fn u4_noop_uninstall_when_no_git_ai(agent: &CatchAllAgent) {
    let (_td, path) = setup_test_env();
    write_settings(
        &path,
        &hooks_for(
            agent.first_hook_type(),
            json!([block("*", &["echo hello"])]),
        ),
    );

    let diff = agent.uninstall(&path);
    assert!(
        diff.is_none(),
        "should return None when nothing to uninstall"
    );
}

// ---- check_hooks scenarios ----

pub fn c1_no_hooks_returns_not_installed(agent: &CatchAllAgent) {
    let (installed, up_to_date) = (agent.hook_status)(&json!({}));
    assert!(!installed);
    assert!(!up_to_date);
}

pub fn c2_git_ai_in_catch_all_returns_up_to_date(agent: &CatchAllAgent) {
    let (hook_type, cmd) = &agent.hook_cmds()[0];
    let settings = hooks_for(hook_type, json!([block("*", &[cmd.as_str()])]));
    let (installed, up_to_date) = (agent.hook_status)(&settings);
    assert!(installed);
    assert!(up_to_date);
}

pub fn c3_git_ai_only_in_old_matcher_returns_installed_but_not_up_to_date(agent: &CatchAllAgent) {
    let (hook_type, cmd) = &agent.hook_cmds()[0];
    let settings = hooks_for(
        hook_type,
        json!([block(agent.old_matcher, &[cmd.as_str()])]),
    );
    let (installed, up_to_date) = (agent.hook_status)(&settings);
    assert!(installed, "should be considered installed");
    assert!(!up_to_date, "should not be up-to-date when on old matcher");
}
