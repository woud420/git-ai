use super::*;
use crate::operations::mdm::paths::{clean_path, normalize_windows_path_for_shell};
use std::fs;
use tempfile::TempDir;

fn setup_test_env() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let settings_path = temp_dir.path().join(".claude").join("settings.json");
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    (temp_dir, settings_path)
}

fn binary_path() -> PathBuf {
    PathBuf::from("/usr/local/bin/git-ai")
}

fn params() -> HookInstallerParams {
    HookInstallerParams {
        binary_path: binary_path(),
    }
}

fn expected_cmd() -> String {
    format!("{} {}", binary_path().display(), CLAUDE_PRE_TOOL_CMD)
}

fn read_settings(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn git_ai_blocks_in(hook_type_array: &[Value]) -> Vec<&Value> {
    hook_type_array
        .iter()
        .filter(|block| {
            block
                .get("hooks")
                .and_then(|h| h.as_array())
                .map(|hooks| {
                    hooks.iter().any(|h| {
                        h.get("command")
                            .and_then(|c| c.as_str())
                            .map(is_git_ai_checkpoint_command)
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        })
        .collect()
}

fn catch_all_block(hook_type_array: &[Value]) -> Option<&Value> {
    hook_type_array.iter().find(|b| {
        b.get("matcher")
            .and_then(|m| m.as_str())
            .map(|m| m == CLAUDE_CATCH_ALL_MATCHER)
            .unwrap_or(false)
    })
}

fn hooks_in_catch_all<'a>(settings: &'a Value, hook_type: &str) -> Vec<&'a Value> {
    let Some(blocks) = settings
        .get("hooks")
        .and_then(|h| h.get(hook_type))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    catch_all_block(blocks)
        .and_then(|b| b.get("hooks").and_then(|h| h.as_array()))
        .map(|v| v.iter().collect())
        .unwrap_or_default()
}

// ---- Install scenarios ----

#[test]
fn s1_fresh_install_creates_catch_all_block() {
    let (_td, path) = setup_test_env();
    // File does not exist yet
    fs::remove_file(&path).ok();

    let diff = ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();
    assert!(diff.is_some(), "should produce a diff");

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let hooks = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(hooks.len(), 1, "{hook_type}: expected 1 hook in catch-all");
        assert_eq!(
            hooks[0].get("command").and_then(|c| c.as_str()).unwrap(),
            expected_cmd()
        );
    }
}

#[test]
fn s2_idempotent_already_on_catch_all() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}]}],
                "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let diff = ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();
    assert!(diff.is_none(), "should return None when already up-to-date");
}

#[test]
fn s3_migration_old_matcher_no_user_hooks() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{"matcher": "Write|Edit|MultiEdit", "hooks": [{"type":"command","command": cmd}]}],
                "PostToolUse": [{"matcher": "Write|Edit|MultiEdit", "hooks": [{"type":"command","command": cmd}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        // git-ai must be in the catch-all block
        let hooks = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(hooks.len(), 1, "{hook_type}: expected git-ai in catch-all");

        // The old matcher block had only our hook, so it must be removed entirely.
        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(
            blocks.len(),
            1,
            "{hook_type}: old matcher block should be removed, only catch-all should remain"
        );
    }
}

#[test]
fn s4_migration_old_matcher_user_hook_preserved() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Write|Edit|MultiEdit",
                    "hooks": [
                        {"type":"command","command": "echo before"},
                        {"type":"command","command": cmd}
                    ]
                }],
                "PostToolUse": [{
                    "matcher": "Write|Edit|MultiEdit",
                    "hooks": [
                        {"type":"command","command": "prettier --write"},
                        {"type":"command","command": cmd}
                    ]
                }]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for (hook_type, user_cmd) in &[
        ("PreToolUse", "echo before"),
        ("PostToolUse", "prettier --write"),
    ] {
        // git-ai in catch-all
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 1);

        // user hook still in old matcher block
        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let old_block = blocks
            .iter()
            .find(|b| b.get("matcher").and_then(|m| m.as_str()) == Some("Write|Edit|MultiEdit"))
            .expect("old matcher block should still exist");
        let old_hooks = old_block.get("hooks").and_then(|h| h.as_array()).unwrap();
        assert!(
            old_hooks
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some(*user_cmd)),
            "{hook_type}: user hook '{user_cmd}' should still be in old matcher block"
        );
        // git-ai NOT in old block
        assert!(
            !old_hooks.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(is_git_ai_checkpoint_command)
                    .unwrap_or(false)
            }),
            "{hook_type}: git-ai should not be in old matcher block after migration"
        );
    }
}

#[test]
fn s5_fresh_install_user_has_old_matcher_hook() {
    let (_td, path) = setup_test_env();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{"matcher": "Write|Edit|MultiEdit", "hooks": [{"type":"command","command": "prettier --write"}]}],
                "PostToolUse": [{"matcher": "Write|Edit|MultiEdit", "hooks": [{"type":"command","command": "prettier --write"}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        // git-ai in new catch-all block
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 1);

        // user hook untouched in old block
        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let old_block = blocks
            .iter()
            .find(|b| b.get("matcher").and_then(|m| m.as_str()) == Some("Write|Edit|MultiEdit"))
            .unwrap();
        let old_hooks = old_block.get("hooks").and_then(|h| h.as_array()).unwrap();
        assert_eq!(old_hooks.len(), 1);
        assert_eq!(
            old_hooks[0]
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap(),
            "prettier --write"
        );
    }
}

#[test]
fn s6_fresh_install_user_has_catch_all_hook() {
    let (_td, path) = setup_test_env();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "my-audit-tool"}]}],
                "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "my-audit-tool"}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(
            catch_all.len(),
            2,
            "{hook_type}: should have user hook + git-ai"
        );
        assert_eq!(
            catch_all[0]
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap(),
            "my-audit-tool",
            "user hook should be first"
        );
        assert!(is_git_ai_checkpoint_command(
            catch_all[1]
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap()
        ));
    }
}

#[test]
fn s7_idempotent_user_catch_all_plus_git_ai() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    let before = json!({
        "hooks": {
            "PreToolUse": [{"matcher": "*", "hooks": [
                {"type":"command","command": "my-audit-tool"},
                {"type":"command","command": cmd}
            ]}],
            "PostToolUse": [{"matcher": "*", "hooks": [
                {"type":"command","command": "my-audit-tool"},
                {"type":"command","command": cmd}
            ]}]
        }
    });
    fs::write(&path, serde_json::to_string_pretty(&before).unwrap()).unwrap();

    let diff = ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();
    assert!(diff.is_none(), "should be idempotent");
}

#[test]
fn s8_deduplication_git_ai_in_both_catch_all_and_old_matcher() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    let user_cmd = "echo user-hook";
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [
                    {"matcher": "*", "hooks": [{"type":"command","command": cmd}]},
                    {"matcher": "Write|Edit|MultiEdit", "hooks": [
                        {"type":"command","command": user_cmd},
                        {"type":"command","command": cmd}
                    ]}
                ],
                "PostToolUse": [
                    {"matcher": "*", "hooks": [{"type":"command","command": cmd}]},
                    {"matcher": "Write|Edit|MultiEdit", "hooks": [
                        {"type":"command","command": user_cmd},
                        {"type":"command","command": cmd}
                    ]}
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        // exactly one git-ai in catch-all
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 1);

        // old matcher block has user hook but NOT git-ai
        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let old_block = blocks
            .iter()
            .find(|b| b.get("matcher").and_then(|m| m.as_str()) == Some("Write|Edit|MultiEdit"))
            .unwrap();
        let old_hooks = old_block.get("hooks").and_then(|h| h.as_array()).unwrap();
        assert!(
            old_hooks
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some(user_cmd))
        );
        assert!(!old_hooks.iter().any(|h| {
            h.get("command")
                .and_then(|c| c.as_str())
                .map(is_git_ai_checkpoint_command)
                .unwrap_or(false)
        }));
    }
}

#[test]
fn s9_deduplication_two_git_ai_in_catch_all_block() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{"matcher": "*", "hooks": [
                    {"type":"command","command": cmd},
                    {"type":"command","command": cmd}
                ]}],
                "PostToolUse": [{"matcher": "*", "hooks": [
                    {"type":"command","command": cmd},
                    {"type":"command","command": cmd}
                ]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(
            catch_all.len(),
            1,
            "{hook_type}: should have exactly 1 after dedup"
        );
    }
}

#[test]
fn s10_stale_command_upgraded_in_catch_all() {
    let (_td, path) = setup_test_env();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "/old/path/git-ai checkpoint claude --hook-input stdin"}]}],
                "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "/old/path/git-ai checkpoint claude --hook-input stdin"}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 1);
        assert_eq!(
            catch_all[0]
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap(),
            expected_cmd()
        );
    }
}

#[test]
fn s11_git_ai_in_arbitrary_old_matcher_migrated() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [
                        {"type":"command","command": "user-bash-hook"},
                        {"type":"command","command": cmd}
                    ]}
                ],
                "PostToolUse": [
                    {"matcher": "Bash", "hooks": [
                        {"type":"command","command": "user-bash-hook"},
                        {"type":"command","command": cmd}
                    ]}
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        // git-ai now in catch-all
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 1);

        // user-bash-hook preserved in Bash block, git-ai removed
        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let bash_block = blocks
            .iter()
            .find(|b| b.get("matcher").and_then(|m| m.as_str()) == Some("Bash"))
            .unwrap();
        let bash_hooks = bash_block.get("hooks").and_then(|h| h.as_array()).unwrap();
        assert!(
            bash_hooks
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some("user-bash-hook"))
        );
        assert!(!bash_hooks.iter().any(|h| {
            h.get("command")
                .and_then(|c| c.as_str())
                .map(is_git_ai_checkpoint_command)
                .unwrap_or(false)
        }));
    }
}

mod migration_and_uninstall;
