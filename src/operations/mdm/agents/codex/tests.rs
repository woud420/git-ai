use super::CODEX_HOOK_EVENTS;
use super::CodexInstaller;
use serde_json::json;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

pub(super) fn test_binary_path() -> PathBuf {
    PathBuf::from("/usr/local/bin/git-ai")
}

pub(super) fn with_temp_home<F: FnOnce(&Path)>(f: F) {
    let temp = tempdir().unwrap();
    let home = temp.path().to_path_buf();

    let prev_home = std::env::var_os("HOME");
    let prev_userprofile = std::env::var_os("USERPROFILE");
    let prev_codex_home = std::env::var_os("CODEX_HOME");

    // SAFETY: tests are serialized via #[serial], so mutating process env is safe.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::remove_var("CODEX_HOME");
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
        match prev_codex_home {
            Some(v) => std::env::set_var("CODEX_HOME", v),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }
}

#[test]
fn test_is_git_ai_codex_notify_args_true_for_absolute_binary() {
    let args = vec![
        "/usr/local/bin/git-ai".to_string(),
        "checkpoint".to_string(),
        "codex".to_string(),
        "--hook-input".to_string(),
    ];

    assert!(CodexInstaller::is_git_ai_codex_notify_args(&args));
}

#[test]
fn test_is_git_ai_codex_notify_args_true_for_legacy_via_codex_notify_args() {
    let args = vec![
        "/Users/svarlamov/.git-ai/bin/git-ai".to_string(),
        "checkpoint".to_string(),
        "codex".to_string(),
        "--via-codex-notify".to_string(),
        "--hook-input".to_string(),
        "stdin".to_string(),
    ];

    assert!(CodexInstaller::is_git_ai_codex_notify_args(&args));
}

#[test]
fn test_is_git_ai_codex_notify_args_false_for_non_git_ai_command() {
    let args = vec![
        "notify-send".to_string(),
        "Codex".to_string(),
        "done".to_string(),
    ];

    assert!(!CodexInstaller::is_git_ai_codex_notify_args(&args));
}

#[test]
fn test_remove_notify_if_git_ai_removes_only_git_ai_notify() {
    let config = CodexInstaller::parse_config_toml(
        r#"
model = "gpt-5"
notify = ["/usr/local/bin/git-ai", "checkpoint", "codex", "--hook-input"]
"#,
    )
    .unwrap();

    let merged = CodexInstaller::remove_notify_if_git_ai(&config)
        .unwrap()
        .expect("notify should be removed");
    assert!(merged.get("notify").is_none());
    assert_eq!(merged.get("model").and_then(|v| v.as_str()), Some("gpt-5"));
}

#[test]
fn test_remove_notify_if_git_ai_removes_legacy_via_codex_notify_args() {
    let config = CodexInstaller::parse_config_toml(
        r#"
model = "gpt-5"
notify = ["/Users/svarlamov/.git-ai/bin/git-ai", "checkpoint", "codex", "--via-codex-notify", "--hook-input", "stdin"]
"#,
    )
    .unwrap();

    let merged = CodexInstaller::remove_notify_if_git_ai(&config)
        .unwrap()
        .expect("legacy git-ai notify should be removed");
    assert!(merged.get("notify").is_none());
    assert_eq!(merged.get("model").and_then(|v| v.as_str()), Some("gpt-5"));
}

#[test]
fn test_remove_notify_if_git_ai_preserves_custom_notify() {
    let config = CodexInstaller::parse_config_toml(
        r#"
model = "gpt-5"
notify = ["notify-send", "Codex"]
"#,
    )
    .unwrap();

    let merged = CodexInstaller::remove_notify_if_git_ai(&config).unwrap();
    assert!(
        merged.is_none(),
        "Custom notify config should remain untouched"
    );
}

#[test]
fn test_config_with_installed_hooks_sets_new_feature_flag_and_inline_hooks() {
    let existing = CodexInstaller::parse_config_toml(
        r#"
model = "gpt-5"
notify = ["/usr/local/bin/git-ai", "checkpoint", "codex", "--hook-input"]
"#,
    )
    .unwrap();

    let merged =
        CodexInstaller::config_with_installed_hooks(&existing, &test_binary_path()).unwrap();
    assert!(CodexInstaller::notify_args_from_config(&merged).is_none());
    assert_eq!(
        merged
            .get("features")
            .and_then(|value| value.get("hooks"))
            .and_then(|value| value.as_bool()),
        Some(true),
        "should use new 'hooks' feature flag"
    );
    assert!(
        merged
            .get("features")
            .and_then(|value| value.get("codex_hooks"))
            .is_none(),
        "legacy codex_hooks flag should be removed"
    );
    assert!(
        CodexInstaller::config_has_inline_hooks(&merged),
        "inline hooks should be present in config"
    );
    assert_eq!(
        merged.get("model").and_then(|value| value.as_str()),
        Some("gpt-5"),
        "other config should be preserved"
    );
}

#[test]
fn test_config_with_installed_hooks_migrates_legacy_codex_hooks_flag() {
    let existing = CodexInstaller::parse_config_toml(
        r#"
model = "gpt-5"

[features]
codex_hooks = true
"#,
    )
    .unwrap();

    let merged =
        CodexInstaller::config_with_installed_hooks(&existing, &test_binary_path()).unwrap();
    assert_eq!(
        merged
            .get("features")
            .and_then(|value| value.get("hooks"))
            .and_then(|value| value.as_bool()),
        Some(true),
        "should use new 'hooks' feature flag"
    );
    assert!(
        merged
            .get("features")
            .and_then(|value| value.get("codex_hooks"))
            .is_none(),
        "legacy codex_hooks flag should be removed"
    );
}

#[test]
fn test_config_with_installed_hooks_adds_inline_hooks_for_all_events() {
    let existing = CodexInstaller::parse_config_toml("model = \"gpt-5\"\n").unwrap();

    let merged =
        CodexInstaller::config_with_installed_hooks(&existing, &test_binary_path()).unwrap();

    let desired_cmd = CodexInstaller::desired_command(&test_binary_path());
    for event_name in CODEX_HOOK_EVENTS {
        let blocks = merged
            .get("hooks")
            .and_then(|h| h.get(event_name))
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("missing hooks.{event_name}"));
        assert!(
            blocks.iter().any(|block| {
                block.get("matcher").is_none()
                    && block
                        .get("hooks")
                        .and_then(|v| v.as_array())
                        .map(|hooks| {
                            hooks.iter().any(|hook| {
                                hook.get("command").and_then(|v| v.as_str())
                                    == Some(desired_cmd.as_str())
                            })
                        })
                        .unwrap_or(false)
            }),
            "expected unscoped git-ai block for {event_name}"
        );
    }
}

#[test]
fn test_config_with_installed_hooks_preserves_existing_matched_hooks() {
    let existing = CodexInstaller::parse_config_toml(
        r#"
model = "gpt-5"

[[hooks.PreToolUse]]
matcher = "Bash"

[[hooks.PreToolUse.hooks]]
type = "command"
command = "echo keep-me"
"#,
    )
    .unwrap();

    let merged =
        CodexInstaller::config_with_installed_hooks(&existing, &test_binary_path()).unwrap();

    let pre_blocks = merged
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(|v| v.as_array())
        .expect("PreToolUse blocks should exist");
    assert!(
        pre_blocks.iter().any(|block| {
            block.get("matcher").and_then(|v| v.as_str()) == Some("Bash")
                && block
                    .get("hooks")
                    .and_then(|v| v.as_array())
                    .map(|hooks| {
                        hooks.iter().any(|hook| {
                            hook.get("command").and_then(|v| v.as_str()) == Some("echo keep-me")
                        })
                    })
                    .unwrap_or(false)
        }),
        "existing matched hooks should be preserved"
    );
}

#[test]
fn test_config_hooks_feature_enabled_detects_new_flag() {
    let config = CodexInstaller::parse_config_toml(
        r#"
[features]
hooks = true
"#,
    )
    .unwrap();
    assert!(CodexInstaller::config_hooks_feature_enabled(&config));
}

#[test]
fn test_config_hooks_feature_enabled_detects_legacy_flag() {
    let config = CodexInstaller::parse_config_toml(
        r#"
[features]
codex_hooks = true
"#,
    )
    .unwrap();
    assert!(CodexInstaller::config_hooks_feature_enabled(&config));
}

#[test]
fn test_remove_inline_hooks_from_config() {
    let config = CodexInstaller::parse_config_toml(
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

    let (merged, changed) = CodexInstaller::remove_inline_hooks_from_config(&config).unwrap();
    assert!(changed);
    assert!(
        merged.get("hooks").is_none(),
        "[hooks] table should be removed when empty"
    );
}

#[test]
fn test_remove_inline_hooks_preserves_non_git_ai_hooks() {
    let config = CodexInstaller::parse_config_toml(
        r#"
model = "gpt-5"

[[hooks.PreToolUse]]

[[hooks.PreToolUse.hooks]]
type = "command"
command = "/usr/local/bin/git-ai checkpoint codex --hook-input stdin"

[[hooks.PreToolUse.hooks]]
type = "command"
command = "echo keep-me"
"#,
    )
    .unwrap();

    let (merged, changed) = CodexInstaller::remove_inline_hooks_from_config(&config).unwrap();
    assert!(changed);
    let pre_blocks = merged
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(|v| v.as_array())
        .expect("PreToolUse should still exist");
    assert_eq!(pre_blocks.len(), 1);
    let hooks_arr = pre_blocks[0]
        .get("hooks")
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(hooks_arr.len(), 1);
    assert_eq!(
        hooks_arr[0].get("command").and_then(|v| v.as_str()),
        Some("echo keep-me")
    );
}

#[test]
fn test_remove_feature_flags_removes_both_old_and_new() {
    let config = CodexInstaller::parse_config_toml(
        r#"
model = "gpt-5"

[features]
hooks = true
codex_hooks = true
"#,
    )
    .unwrap();

    let merged = CodexInstaller::remove_feature_flags(&config).unwrap();
    assert!(
        merged.get("features").is_none(),
        "features section should be removed when empty"
    );
}

#[test]
fn test_remove_codex_hooks_from_json_removes_only_git_ai_entries() {
    let existing = json!({
        "hooks": {
            "PreToolUse": [
                {
                    "hooks": [
                        { "type": "command", "command": "/usr/local/bin/git-ai checkpoint codex --hook-input stdin" },
                        { "type": "command", "command": "echo keep" }
                    ]
                }
            ],
            "Stop": [
                {
                    "hooks": [
                        { "type": "command", "command": "/usr/local/bin/git-ai checkpoint codex --hook-input stdin" }
                    ]
                }
            ]
        }
    });

    let (merged, changed) = CodexInstaller::remove_codex_hooks_from_json(&existing).unwrap();
    assert!(changed);
    assert_eq!(
        merged["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str(),
        Some("echo keep")
    );
    assert!(
        merged["hooks"].get("Stop").is_none(),
        "empty event arrays should be removed"
    );
}

#[test]
fn test_parse_config_toml_malformed() {
    let result = CodexInstaller::parse_config_toml("invalid [[ toml");
    assert!(result.is_err(), "Malformed TOML should return Err");
}

#[test]
fn test_parse_config_toml_non_table_root() {
    let result = CodexInstaller::parse_config_toml("42");
    assert!(result.is_err(), "Non-table root value should return Err");
}

#[test]
fn test_compute_trust_hash_deterministic() {
    let hash1 = CodexInstaller::compute_trust_hash(
        "pre_tool_use",
        "/usr/local/bin/git-ai checkpoint codex --hook-input stdin",
    )
    .unwrap();
    let hash2 = CodexInstaller::compute_trust_hash(
        "pre_tool_use",
        "/usr/local/bin/git-ai checkpoint codex --hook-input stdin",
    )
    .unwrap();
    assert_eq!(hash1, hash2);
    assert!(hash1.starts_with("sha256:"));
    assert_eq!(hash1.len(), 7 + 64); // "sha256:" + 64 hex chars
}

#[test]
fn test_compute_trust_hash_differs_by_event() {
    let hash_pre = CodexInstaller::compute_trust_hash(
        "pre_tool_use",
        "/usr/local/bin/git-ai checkpoint codex --hook-input stdin",
    )
    .unwrap();
    let hash_post = CodexInstaller::compute_trust_hash(
        "post_tool_use",
        "/usr/local/bin/git-ai checkpoint codex --hook-input stdin",
    )
    .unwrap();
    let hash_stop = CodexInstaller::compute_trust_hash(
        "stop",
        "/usr/local/bin/git-ai checkpoint codex --hook-input stdin",
    )
    .unwrap();
    assert_ne!(hash_pre, hash_post);
    assert_ne!(hash_pre, hash_stop);
    assert_ne!(hash_post, hash_stop);
}

#[test]
fn test_compute_trust_hash_differs_by_command() {
    let hash1 = CodexInstaller::compute_trust_hash(
        "pre_tool_use",
        "/usr/local/bin/git-ai checkpoint codex --hook-input stdin",
    )
    .unwrap();
    let hash2 = CodexInstaller::compute_trust_hash(
        "pre_tool_use",
        "/opt/bin/git-ai checkpoint codex --hook-input stdin",
    )
    .unwrap();
    assert_ne!(hash1, hash2);
}

#[test]
fn test_canonical_json_sorts_keys() {
    use serde_json::Value as JsonValue;
    let input: JsonValue = serde_json::json!({
        "z_key": 1,
        "a_key": 2,
        "m_key": {"b": 1, "a": 2}
    });
    let result = CodexInstaller::canonical_json(&input);
    let serialized = serde_json::to_string(&result).unwrap();
    assert_eq!(serialized, r#"{"a_key":2,"m_key":{"a":2,"b":1},"z_key":1}"#);
}
