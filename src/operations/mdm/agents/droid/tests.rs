use super::*;
use crate::operations::mdm::test_env::{with_fake_binary_on_path, with_temp_home};
use serial_test::serial;
use std::fs;
use tempfile::TempDir;

fn setup_test_env() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let settings_path = temp_dir.path().join(".factory").join("settings.json");
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
    format!("{} {}", binary_path().display(), DROID_PRE_TOOL_CMD)
}

fn read_settings(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn hooks_in_catch_all<'a>(settings: &'a Value, hook_type: &str) -> Vec<&'a Value> {
    let Some(blocks) = settings
        .get("hooks")
        .and_then(|h| h.get(hook_type))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    blocks
        .iter()
        .find(|b| {
            b.get("matcher")
                .and_then(|m| m.as_str())
                .map(|m| m == DROID_CATCH_ALL_MATCHER)
                .unwrap_or(false)
        })
        .and_then(|b| b.get("hooks").and_then(|h| h.as_array()))
        .map(|v| v.iter().collect())
        .unwrap_or_default()
}

// ---- Install scenarios ----

#[test]
fn s1_fresh_install_creates_catch_all_block() {
    let (_td, path) = setup_test_env();
    fs::remove_file(&path).ok();

    let diff = DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();
    assert!(diff.is_some());

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let hooks = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(hooks.len(), 1, "{hook_type}: expected 1 hook in catch-all");
        assert_eq!(
            hooks[0].get("command").and_then(|c| c.as_str()).unwrap(),
            expected_cmd()
        );
    }
    // claudeHooksImported flag should be set
    assert_eq!(
        settings
            .get("hooks")
            .and_then(|h| h.get("claudeHooksImported")),
        Some(&json!(true))
    );
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
                "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}]}],
                "claudeHooksImported": true
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let diff = DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();
    assert!(diff.is_none(), "should be idempotent");
}

#[test]
fn s3_migration_old_matcher_no_user_hooks() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
            &path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [{"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [{"type":"command","command": cmd}]}],
                    "PostToolUse": [{"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [{"type":"command","command": cmd}]}]
                }
            }))
            .unwrap(),
        )
        .unwrap();

    DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let hooks = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(hooks.len(), 1, "{hook_type}: git-ai should be in catch-all");

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
                "PreToolUse": [{"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [
                    {"type":"command","command": "echo before"},
                    {"type":"command","command": cmd}
                ]}],
                "PostToolUse": [{"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [
                    {"type":"command","command": "prettier --write"},
                    {"type":"command","command": cmd}
                ]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for (hook_type, user_cmd) in &[
        ("PreToolUse", "echo before"),
        ("PostToolUse", "prettier --write"),
    ] {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 1);

        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let old_block = blocks
            .iter()
            .find(|b| {
                b.get("matcher").and_then(|m| m.as_str())
                    == Some("^(Edit|Write|Create|ApplyPatch)$")
            })
            .unwrap();
        let old_hooks = old_block.get("hooks").and_then(|h| h.as_array()).unwrap();
        assert!(
            old_hooks
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some(*user_cmd))
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
fn s5_fresh_install_user_has_old_matcher_hook() {
    let (_td, path) = setup_test_env();
    fs::write(
            &path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [{"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [{"type":"command","command": "echo user"}]}],
                    "PostToolUse": [{"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [{"type":"command","command": "echo user"}]}]
                }
            }))
            .unwrap(),
        )
        .unwrap();

    DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 1);

        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let old_block = blocks
            .iter()
            .find(|b| {
                b.get("matcher").and_then(|m| m.as_str())
                    == Some("^(Edit|Write|Create|ApplyPatch)$")
            })
            .unwrap();
        let old_hooks = old_block.get("hooks").and_then(|h| h.as_array()).unwrap();
        assert_eq!(old_hooks.len(), 1);
        assert_eq!(
            old_hooks[0]
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap(),
            "echo user"
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
                    "PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "audit-tool"}]}],
                    "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "audit-tool"}]}]
                }
            }))
            .unwrap(),
        )
        .unwrap();

    DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 2);
        assert_eq!(
            catch_all[0]
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap(),
            "audit-tool"
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
            "PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "audit-tool"}, {"type":"command","command": cmd}]}],
            "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "audit-tool"}, {"type":"command","command": cmd}]}],
            "claudeHooksImported": true
        }
    });
    fs::write(&path, serde_json::to_string_pretty(&before).unwrap()).unwrap();
    let diff = DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();
    assert!(diff.is_none());
}

#[test]
fn s8_deduplication_git_ai_in_both_blocks() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
            &path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [
                        {"matcher": "*", "hooks": [{"type":"command","command": cmd}]},
                        {"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [{"type":"command","command": "user"}, {"type":"command","command": cmd}]}
                    ],
                    "PostToolUse": [
                        {"matcher": "*", "hooks": [{"type":"command","command": cmd}]},
                        {"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [{"type":"command","command": "user"}, {"type":"command","command": cmd}]}
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();

    DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 1);

        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let old_block = blocks
            .iter()
            .find(|b| {
                b.get("matcher").and_then(|m| m.as_str())
                    == Some("^(Edit|Write|Create|ApplyPatch)$")
            })
            .unwrap();
        let old_hooks = old_block.get("hooks").and_then(|h| h.as_array()).unwrap();
        assert!(
            old_hooks
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some("user"))
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
fn s9_deduplication_two_git_ai_in_catch_all() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
            &path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}, {"type":"command","command": cmd}]}],
                    "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}, {"type":"command","command": cmd}]}],
                    "claudeHooksImported": true
                }
            }))
            .unwrap(),
        )
        .unwrap();

    DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert_eq!(catch_all.len(), 1);
    }
}

#[test]
fn s10_stale_command_upgraded() {
    let (_td, path) = setup_test_env();
    fs::write(
            &path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "/old/git-ai checkpoint droid"}]}],
                    "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "/old/git-ai checkpoint droid"}]}],
                    "claudeHooksImported": true
                }
            }))
            .unwrap(),
        )
        .unwrap();

    DroidInstaller::install_hooks_at(&path, &params(), false).unwrap();

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

// ---- Uninstall scenarios ----

#[test]
fn u1_uninstall_from_catch_all() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}]}],
                "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}]}],
                "claudeHooksImported": true
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let diff = DroidInstaller::uninstall_hooks_at(&path, false).unwrap();
    assert!(diff.is_some());

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert!(!catch_all.iter().any(|h| {
            h.get("command")
                .and_then(|c| c.as_str())
                .map(is_git_ai_checkpoint_command)
                .unwrap_or(false)
        }));
    }
}

#[test]
fn u2_uninstall_from_old_matcher_preserves_user_hook() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [
                    {"type":"command","command": "echo before"},
                    {"type":"command","command": cmd}
                ]}],
                "PostToolUse": [{"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [
                    {"type":"command","command": "echo before"},
                    {"type":"command","command": cmd}
                ]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    DroidInstaller::uninstall_hooks_at(&path, false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let old_block = blocks
            .iter()
            .find(|b| {
                b.get("matcher").and_then(|m| m.as_str())
                    == Some("^(Edit|Write|Create|ApplyPatch)$")
            })
            .unwrap();
        let hooks = old_block.get("hooks").and_then(|h| h.as_array()).unwrap();
        assert!(
            hooks
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some("echo before"))
        );
        assert!(!hooks.iter().any(|h| {
            h.get("command")
                .and_then(|c| c.as_str())
                .map(is_git_ai_checkpoint_command)
                .unwrap_or(false)
        }));
    }
}

#[test]
fn u3_uninstall_from_multiple_blocks() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    let user = "echo user";
    fs::write(
            &path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [
                        {"matcher": "*", "hooks": [{"type":"command","command": cmd}, {"type":"command","command": user}]},
                        {"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [{"type":"command","command": cmd}]}
                    ],
                    "PostToolUse": [
                        {"matcher": "*", "hooks": [{"type":"command","command": cmd}]},
                        {"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [{"type":"command","command": cmd}, {"type":"command","command": user}]}
                    ],
                    "claudeHooksImported": true
                }
            }))
            .unwrap(),
        )
        .unwrap();

    DroidInstaller::uninstall_hooks_at(&path, false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let all_blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        for block in all_blocks {
            let empty_hooks: Vec<Value> = Vec::new();
            let hooks = block
                .get("hooks")
                .and_then(|h| h.as_array())
                .unwrap_or(&empty_hooks);
            assert!(!hooks.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(is_git_ai_checkpoint_command)
                    .unwrap_or(false)
            }));
        }
    }
}

#[test]
fn u4_noop_uninstall_when_no_git_ai() {
    let (_td, path) = setup_test_env();
    fs::write(
            &path,
            serde_json::to_string_pretty(&json!({"hooks": {"PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "echo hello"}]}]}}))
                .unwrap(),
        )
        .unwrap();

    let diff = DroidInstaller::uninstall_hooks_at(&path, false).unwrap();
    assert!(diff.is_none());
}

// ---- check_hooks scenarios ----

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
    assert_eq!(
        catch_all[0]
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap(),
        "echo existing"
    );
    assert!(is_git_ai_checkpoint_command(
        catch_all[1]
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap()
    ));
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

// ---- check_hooks scenarios ----

#[test]
fn c1_no_hooks_returns_not_installed() {
    let (installed, up_to_date) = DroidInstaller::hook_status(&json!({}));
    assert!(!installed);
    assert!(!up_to_date);
}

#[test]
fn c2_git_ai_in_catch_all_returns_up_to_date() {
    let cmd = expected_cmd();
    let settings = json!({"hooks": {"PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}]}]}});
    let (installed, up_to_date) = DroidInstaller::hook_status(&settings);
    assert!(installed);
    assert!(up_to_date);
}

#[test]
fn c3_git_ai_only_in_old_matcher_not_up_to_date() {
    let cmd = expected_cmd();
    let settings = json!({"hooks": {"PreToolUse": [{"matcher": "^(Edit|Write|Create|ApplyPatch)$", "hooks": [{"type":"command","command": cmd}]}]}});
    let (installed, up_to_date) = DroidInstaller::hook_status(&settings);
    assert!(installed);
    assert!(!up_to_date);
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
