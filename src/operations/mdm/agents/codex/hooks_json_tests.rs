use super::CodexInstaller;
use super::tests::{test_binary_path, with_temp_home};
use crate::operations::mdm::hook_installer::{HookInstaller, HookInstallerParams};
use serde_json::json;
use serial_test::serial;
use std::fs;
use std::path::Path;

fn write_git_ai_config(home: &Path, codex_hooks_format: &str) {
    let git_ai_dir = home.join(".git-ai");
    fs::create_dir_all(&git_ai_dir).unwrap();
    fs::write(
        git_ai_dir.join("config.json"),
        serde_json::to_string_pretty(&json!({
            "codex_hooks_format": codex_hooks_format
        }))
        .unwrap(),
    )
    .unwrap();
}

// --- hooks_json format mode: install ---

#[test]
#[serial]
fn test_install_hooks_prefers_hooks_json_when_configured() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        write_git_ai_config(home, "hooks_json");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        let hooks_json_path = codex_dir.join("hooks.json");
        fs::write(&config_path, "model = \"gpt-5\"\n").unwrap();
        fs::write(
            &hooks_json_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "echo keep" }] }]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        installer
            .install_hooks(&params, false)
            .expect("install should succeed");

        let config_content = fs::read_to_string(&config_path).unwrap();
        let parsed = CodexInstaller::parse_config_toml(&config_content).unwrap();
        assert_eq!(
            parsed
                .get("features")
                .and_then(|v| v.get("hooks"))
                .and_then(|v| v.as_bool()),
            Some(true),
            "install should still enable Codex hooks"
        );
        assert!(
            !CodexInstaller::config_has_inline_hooks(&parsed),
            "configured hooks_json format should not install git-ai inline hooks"
        );

        let hooks_content = fs::read_to_string(&hooks_json_path).unwrap();
        let hooks_json: serde_json::Value = serde_json::from_str(&hooks_content).unwrap();
        assert!(
            CodexInstaller::hooks_json_has_git_ai_entries(&hooks_json),
            "git-ai hooks should be installed into hooks.json"
        );
        let pre_blocks = hooks_json["hooks"]["PreToolUse"].as_array().unwrap();
        assert!(
            pre_blocks.iter().any(|block| {
                block["hooks"].as_array().is_some_and(|hooks| {
                    hooks
                        .iter()
                        .any(|hook| hook["command"].as_str() == Some("echo keep"))
                })
            }),
            "existing hooks.json entries should be preserved"
        );

        let check = installer
            .check_hooks(&params)
            .expect("check should succeed");
        assert!(check.tool_installed);
        assert!(check.hooks_installed);
        assert!(check.hooks_up_to_date);
    });
}

#[test]
#[serial]
fn test_install_hooks_prefers_hooks_json_removes_existing_inline_git_ai_hooks() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        write_git_ai_config(home, "hooks_json");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        let hooks_json_path = codex_dir.join("hooks.json");
        fs::write(
            &config_path,
            r#"
model = "gpt-5"

[features]
hooks = true

[[hooks.PreToolUse]]

[[hooks.PreToolUse.hooks]]
type = "command"
command = "/usr/local/bin/git-ai checkpoint codex --hook-input stdin"

[[hooks.PreToolUse.hooks]]
type = "command"
command = "echo keep-inline"

[[hooks.PostToolUse]]

[[hooks.PostToolUse.hooks]]
type = "command"
command = "/usr/local/bin/git-ai checkpoint codex --hook-input stdin"

[[hooks.Stop]]

[[hooks.Stop.hooks]]
type = "command"
command = "/usr/local/bin/git-ai checkpoint codex --hook-input stdin"
"#,
        )
        .unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        installer
            .install_hooks(&params, false)
            .expect("install should succeed");

        let config_content = fs::read_to_string(&config_path).unwrap();
        let parsed = CodexInstaller::parse_config_toml(&config_content).unwrap();
        assert!(
            !CodexInstaller::config_has_inline_hooks(&parsed),
            "old git-ai inline hooks should be removed in hooks_json mode"
        );
        let pre_blocks = parsed
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .expect("custom PreToolUse block should remain");
        assert!(
            pre_blocks.iter().any(|block| {
                block
                    .get("hooks")
                    .and_then(|v| v.as_array())
                    .is_some_and(|hooks| {
                        hooks.iter().any(|hook| {
                            hook.get("command").and_then(|v| v.as_str()) == Some("echo keep-inline")
                        })
                    })
            }),
            "non-git-ai inline hooks should be preserved"
        );

        let hooks_content = fs::read_to_string(&hooks_json_path).unwrap();
        let hooks_json: serde_json::Value = serde_json::from_str(&hooks_content).unwrap();
        assert!(
            CodexInstaller::hooks_json_has_git_ai_entries(&hooks_json),
            "git-ai hooks should be installed into hooks.json"
        );

        let check = installer
            .check_hooks(&params)
            .expect("check should succeed");
        assert!(check.hooks_installed);
        assert!(check.hooks_up_to_date);
    });
}

// --- Uninstall ---

#[test]
#[serial]
fn test_uninstall_hooks_removes_inline_hooks_and_feature_flags() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(
            &config_path,
            r#"
model = "gpt-5"

[features]
hooks = true

[[hooks.PreToolUse]]

[[hooks.PreToolUse.hooks]]
type = "command"
command = "/usr/local/bin/git-ai checkpoint codex --hook-input stdin"

[[hooks.PostToolUse]]

[[hooks.PostToolUse.hooks]]
type = "command"
command = "/usr/local/bin/git-ai checkpoint codex --hook-input stdin"

[[hooks.Stop]]

[[hooks.Stop.hooks]]
type = "command"
command = "/usr/local/bin/git-ai checkpoint codex --hook-input stdin"
"#,
        )
        .unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let diff = installer
            .uninstall_hooks(&params, false)
            .expect("uninstall should succeed");
        assert!(diff.is_some(), "uninstall should report a diff");

        let parsed =
            CodexInstaller::parse_config_toml(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert!(
            parsed.get("features").is_none(),
            "features section should be removed"
        );
        assert!(
            parsed.get("hooks").is_none(),
            "hooks section should be removed"
        );
        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()),
            Some("gpt-5"),
            "other config should be preserved"
        );
    });
}

#[test]
#[serial]
fn test_uninstall_hooks_removes_legacy_hooks_json() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        let hooks_json_path = codex_dir.join("hooks.json");
        fs::write(
            &config_path,
            r#"
model = "gpt-5"
[features]
codex_hooks = true
"#,
        )
        .unwrap();
        fs::write(
            &hooks_json_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [{ "hooks": [{ "type": "command", "command": "/usr/local/bin/git-ai checkpoint codex --hook-input stdin" }] }],
                    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "/usr/local/bin/git-ai checkpoint codex --hook-input stdin" }] }],
                    "Stop": [{ "hooks": [{ "type": "command", "command": "/usr/local/bin/git-ai checkpoint codex --hook-input stdin" }] }],
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let diff = installer
            .uninstall_hooks(&params, false)
            .expect("uninstall should succeed");
        assert!(diff.is_some(), "uninstall should report a diff");

        let parsed =
            CodexInstaller::parse_config_toml(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert!(
            parsed.get("features").is_none(),
            "feature flags should be removed"
        );
        assert!(
            !hooks_json_path.exists(),
            "hooks.json should be removed when only git-ai entries existed"
        );
    });
}

// --- Check hooks ---

#[test]
#[serial]
fn test_check_hooks_detects_legacy_hooks_json_installation() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        let hooks_json_path = codex_dir.join("hooks.json");
        fs::write(
            &config_path,
            r#"
model = "gpt-5"
[features]
codex_hooks = true
"#,
        )
        .unwrap();
        fs::write(
            &hooks_json_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [{ "hooks": [{ "type": "command", "command": "/usr/local/bin/git-ai checkpoint codex --hook-input stdin" }] }],
                    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "/usr/local/bin/git-ai checkpoint codex --hook-input stdin" }] }],
                    "Stop": [{ "hooks": [{ "type": "command", "command": "/usr/local/bin/git-ai checkpoint codex --hook-input stdin" }] }],
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let check = installer
            .check_hooks(&params)
            .expect("check should succeed");
        assert!(check.tool_installed);
        assert!(
            check.hooks_installed,
            "should detect legacy hooks.json installation"
        );
        assert!(
            !check.hooks_up_to_date,
            "legacy format should not be considered up-to-date"
        );
    });
}

// --- Trust state ---

#[test]
#[serial]
fn test_install_hooks_writes_trust_state() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(&config_path, "model = \"o3\"\n").unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        installer
            .install_hooks(&params, false)
            .expect("install should succeed");

        let content = fs::read_to_string(&config_path).unwrap();
        let parsed = CodexInstaller::parse_config_toml(&content).unwrap();

        let state = parsed
            .get("hooks")
            .and_then(|v| v.get("state"))
            .and_then(|v| v.as_table())
            .expect("hooks.state should exist");

        let config_path_str = config_path.to_string_lossy().to_string();
        for event in ["pre_tool_use", "post_tool_use", "stop"] {
            let key = format!("{config_path_str}:{event}:0:0");
            let entry = state
                .get(&key)
                .and_then(|v| v.as_table())
                .unwrap_or_else(|| panic!("state entry for {event} should exist"));
            assert_eq!(entry.get("enabled").and_then(|v| v.as_bool()), Some(true));
            let hash = entry
                .get("trusted_hash")
                .and_then(|v| v.as_str())
                .expect("trusted_hash should exist");
            assert!(
                hash.starts_with("sha256:"),
                "hash should have sha256 prefix"
            );
            assert_eq!(hash.len(), 71, "hash should be sha256: + 64 hex chars");
        }
    });
}

#[test]
#[serial]
fn test_uninstall_hooks_removes_trust_state() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(&config_path, "model = \"o3\"\n").unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        installer
            .install_hooks(&params, false)
            .expect("install should succeed");

        // Verify state exists before uninstall
        let content = fs::read_to_string(&config_path).unwrap();
        let parsed = CodexInstaller::parse_config_toml(&content).unwrap();
        assert!(
            parsed
                .get("hooks")
                .and_then(|v| v.get("state"))
                .and_then(|v| v.as_table())
                .is_some(),
            "state should exist after install"
        );

        installer
            .uninstall_hooks(&params, false)
            .expect("uninstall should succeed");

        let content = fs::read_to_string(&config_path).unwrap();
        let parsed = CodexInstaller::parse_config_toml(&content).unwrap();
        assert!(
            parsed.get("hooks").is_none(),
            "hooks table should be removed after uninstall"
        );
    });
}

#[test]
#[serial]
fn test_install_preserves_existing_state_entries() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(
            &config_path,
            r#"
model = "o3"

[hooks.state."/some/other/hooks.json:pre_tool_use:0:0"]
enabled = true
trusted_hash = "sha256:abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
"#,
        )
        .unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        installer
            .install_hooks(&params, false)
            .expect("install should succeed");

        let content = fs::read_to_string(&config_path).unwrap();
        let parsed = CodexInstaller::parse_config_toml(&content).unwrap();
        let state = parsed
            .get("hooks")
            .and_then(|v| v.get("state"))
            .and_then(|v| v.as_table())
            .expect("hooks.state should exist");

        // Existing state entry should be preserved
        assert!(
            state.contains_key("/some/other/hooks.json:pre_tool_use:0:0"),
            "non-git-ai state entry should be preserved"
        );

        // Our state entries should also be present
        let config_path_str = config_path.to_string_lossy().to_string();
        assert!(
            state.contains_key(&format!("{config_path_str}:pre_tool_use:0:0")),
            "git-ai state entry should be added"
        );
    });
}

#[test]
#[serial]
fn test_install_hooks_trust_state_idempotent() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(&config_path, "model = \"o3\"\n").unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        installer
            .install_hooks(&params, false)
            .expect("first install should succeed");
        let content_after_first = fs::read_to_string(&config_path).unwrap();

        // Second install should be a no-op (returns None)
        let diff = installer
            .install_hooks(&params, false)
            .expect("second install should succeed");
        assert!(diff.is_none(), "second install should be a no-op");

        let content_after_second = fs::read_to_string(&config_path).unwrap();
        assert_eq!(content_after_first, content_after_second);
    });
}
