use super::*;

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
