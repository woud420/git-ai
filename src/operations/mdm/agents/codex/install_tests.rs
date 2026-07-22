use super::CodexInstaller;
use super::tests::{test_binary_path, with_temp_home};
use crate::operations::mdm::hook_installer::{HookInstaller, HookInstallerParams};
use serde_json::json;
use serial_test::serial;
use std::fs;

#[test]
#[serial]
fn test_install_hooks_writes_inline_toml_and_check_reports_up_to_date() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(&config_path, "model = \"gpt-5\"\n").unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let diff = installer
            .install_hooks(&params, false)
            .expect("install should succeed");
        assert!(diff.is_some(), "install should report a config diff");

        let content = fs::read_to_string(&config_path).unwrap();
        let parsed = CodexInstaller::parse_config_toml(&content).unwrap();
        assert_eq!(
            parsed
                .get("features")
                .and_then(|value| value.get("hooks"))
                .and_then(|value| value.as_bool()),
            Some(true),
            "should set [features].hooks = true"
        );
        assert!(
            CodexInstaller::config_has_inline_hooks(&parsed),
            "inline hooks should be in config.toml"
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
fn test_install_hooks_respects_codex_home() {
    with_temp_home(|home| {
        let default_codex_dir = home.join(".codex");
        let custom_codex_home = home.join("custom-codex-home");
        fs::create_dir_all(&custom_codex_home).unwrap();

        // SAFETY: tests are serialized via #[serial], so mutating process env is safe.
        unsafe {
            std::env::set_var("CODEX_HOME", &custom_codex_home);
        }

        let config_path = custom_codex_home.join("config.toml");
        fs::write(&config_path, "model = \"gpt-5\"\n").unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let diff = installer
            .install_hooks(&params, false)
            .expect("install should succeed");
        assert!(diff.is_some(), "install should report a config diff");
        assert!(
            !default_codex_dir.exists(),
            "default ~/.codex should not be touched when CODEX_HOME is set"
        );

        let content = fs::read_to_string(&config_path).unwrap();
        let parsed = CodexInstaller::parse_config_toml(&content).unwrap();
        assert!(
            CodexInstaller::config_has_inline_hooks(&parsed),
            "inline hooks should be in CODEX_HOME/config.toml"
        );

        let state = parsed
            .get("hooks")
            .and_then(|hooks| hooks.get("state"))
            .and_then(|state| state.as_table())
            .expect("hook trust state should be written");
        let config_path_str = config_path.to_string_lossy();
        assert!(
            state
                .keys()
                .all(|key| key.starts_with(config_path_str.as_ref())),
            "trust state keys should reference CODEX_HOME/config.toml"
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
fn test_install_hooks_migrates_notify_and_sets_new_feature_flag() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(
            &config_path,
            r#"
model = "gpt-5"
notify = ["/usr/local/bin/git-ai", "checkpoint", "codex", "--hook-input"]
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
            CodexInstaller::notify_args_from_config(&parsed).is_none(),
            "git-ai notify should be removed during migration"
        );
        assert_eq!(
            parsed
                .get("features")
                .and_then(|v| v.get("hooks"))
                .and_then(|v| v.as_bool()),
            Some(true),
            "install should use new hooks feature flag"
        );
        assert!(
            CodexInstaller::config_has_inline_hooks(&parsed),
            "hooks should be inline in config.toml"
        );
    });
}

#[test]
#[serial]
fn test_install_hooks_migrates_legacy_via_codex_notify() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(
            &config_path,
            r#"
model = "gpt-5"
notify = ["/Users/svarlamov/.git-ai/bin/git-ai", "checkpoint", "codex", "--via-codex-notify", "--hook-input", "stdin"]
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
            CodexInstaller::notify_args_from_config(&parsed).is_none(),
            "legacy git-ai notify should be removed during migration"
        );
    });
}

#[test]
#[serial]
fn test_install_hooks_preserves_custom_notify() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(
            &config_path,
            r#"
model = "gpt-5"
notify = ["notify-send", "Codex finished"]
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
        assert_eq!(
            CodexInstaller::notify_args_from_config(&parsed),
            Some(vec![
                "notify-send".to_string(),
                "Codex finished".to_string()
            ]),
            "non-git-ai notify must be preserved"
        );
        assert_eq!(
            parsed
                .get("features")
                .and_then(|v| v.get("hooks"))
                .and_then(|v| v.as_bool()),
            Some(true),
            "install should still enable hooks feature flag"
        );
        assert!(
            CodexInstaller::config_has_inline_hooks(&parsed),
            "hooks should be inline in config.toml"
        );
    });
}

#[test]
#[serial]
fn test_install_hooks_dry_run() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        let original_content = "model = \"gpt-5\"\n";
        fs::write(&config_path, original_content).unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let diff = installer
            .install_hooks(&params, true)
            .expect("dry-run install should succeed");
        assert!(diff.is_some(), "dry-run should still produce a diff");

        let after = fs::read_to_string(&config_path).unwrap();
        assert_eq!(
            after, original_content,
            "File should remain unchanged after dry-run install"
        );
    });
}

#[test]
#[serial]
fn test_install_hooks_idempotent() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(&config_path, "model = \"gpt-5\"\n").unwrap();

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let first = installer
            .install_hooks(&params, false)
            .expect("first install should succeed");
        assert!(first.is_some(), "first install should report changes");

        let second = installer
            .install_hooks(&params, false)
            .expect("second install should succeed");
        assert!(
            second.is_none(),
            "second install should return None (no changes needed)"
        );

        let content = fs::read_to_string(&config_path).unwrap();
        let parsed = CodexInstaller::parse_config_toml(&content).unwrap();
        assert!(CodexInstaller::config_has_inline_hooks(&parsed));
    });
}

#[test]
#[serial]
fn test_install_hooks_migrates_hooks_json_to_inline_toml() {
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
                    "PreToolUse": [{ "hooks": [{ "type": "command", "command": "/old/git-ai checkpoint codex --hook-input stdin" }] }],
                    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "/old/git-ai checkpoint codex --hook-input stdin" }] }],
                    "Stop": [{ "hooks": [{ "type": "command", "command": "/old/git-ai checkpoint codex --hook-input stdin" }] }]
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
            "should migrate to new hooks feature flag"
        );
        assert!(
            parsed
                .get("features")
                .and_then(|v| v.get("codex_hooks"))
                .is_none(),
            "legacy codex_hooks flag should be removed"
        );
        assert!(
            CodexInstaller::config_has_inline_hooks(&parsed),
            "hooks should now be inline in config.toml"
        );
        assert!(
            !hooks_json_path.exists(),
            "hooks.json should be removed after migration (no other entries)"
        );
    });
}

#[test]
#[serial]
fn test_install_hooks_migrates_hooks_json_preserves_non_git_ai_entries() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        let hooks_json_path = codex_dir.join("hooks.json");
        fs::write(&config_path, "model = \"gpt-5\"\n").unwrap();
        fs::write(
            &hooks_json_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [{
                        "matcher": "Bash",
                        "hooks": [
                            { "type": "command", "command": "/old/git-ai checkpoint codex --hook-input stdin" },
                            { "type": "command", "command": "echo keep" }
                        ]
                    }]
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

        assert!(
            hooks_json_path.exists(),
            "hooks.json should be preserved when non-git-ai entries remain"
        );
        let hooks_content = fs::read_to_string(&hooks_json_path).unwrap();
        let hooks_json: serde_json::Value = serde_json::from_str(&hooks_content).unwrap();
        let pre_blocks = hooks_json["hooks"]["PreToolUse"].as_array().unwrap();
        assert!(
            pre_blocks.iter().any(|block| {
                block["hooks"].as_array().is_some_and(|hooks| {
                    hooks
                        .iter()
                        .any(|hook| hook["command"].as_str() == Some("echo keep"))
                })
            }),
            "non-git-ai hooks should be preserved in hooks.json"
        );
        assert!(
            !pre_blocks.iter().any(|block| {
                block["hooks"].as_array().is_some_and(|hooks| {
                    hooks.iter().any(|hook| {
                        hook["command"]
                            .as_str()
                            .map(CodexInstaller::is_git_ai_codex_command)
                            .unwrap_or(false)
                    })
                })
            }),
            "git-ai hooks should be removed from hooks.json"
        );
    });
}

#[test]
#[serial]
fn test_install_hooks_creates_missing_codex_dir() {
    with_temp_home(|home| {
        let codex_dir = home.join(".codex");
        assert!(!codex_dir.exists());

        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let result = installer.install_hooks(&params, false).unwrap();
        assert!(result.is_some(), "should report changes for fresh install");

        let config_path = codex_dir.join("config.toml");
        assert!(config_path.exists(), "config.toml should be created");

        let content = fs::read_to_string(&config_path).unwrap();
        let parsed = CodexInstaller::parse_config_toml(&content).unwrap();
        assert!(
            CodexInstaller::config_has_inline_hooks(&parsed),
            "config.toml should contain inline hooks"
        );
        assert_eq!(
            parsed
                .get("features")
                .and_then(|v| v.get("hooks"))
                .and_then(|v| v.as_bool()),
            Some(true),
            "should set hooks feature flag"
        );
    });
}
