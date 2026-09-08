use super::*;

#[test]
fn s12_git_ai_spread_across_multiple_old_blocks() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Write|Edit|MultiEdit", "hooks": [{"type":"command","command": cmd}]},
                    {"matcher": "Bash", "hooks": [{"type":"command","command": cmd}]}
                ],
                "PostToolUse": [
                    {"matcher": "Write|Edit|MultiEdit", "hooks": [{"type":"command","command": cmd}]},
                    {"matcher": "Bash", "hooks": [{"type":"command","command": cmd}]}
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        // exactly one git-ai total, in catch-all
        let all_blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let git_ai_blocks = git_ai_blocks_in(all_blocks);
        assert_eq!(git_ai_blocks.len(), 1);
        assert_eq!(
            git_ai_blocks[0]
                .get("matcher")
                .and_then(|m| m.as_str())
                .unwrap(),
            CLAUDE_CATCH_ALL_MATCHER
        );
    }
}

#[test]
fn s13_hook_types_handled_independently() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    // PreToolUse: git-ai on old matcher; PostToolUse: git-ai already on catch-all
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{"matcher": "Write|Edit|MultiEdit", "hooks": [{"type":"command","command": cmd}]}],
                "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::install_hooks_at(&path, &params(), false).unwrap();

    let settings = read_settings(&path);

    // PreToolUse: migrated to catch-all
    let pre_catch = hooks_in_catch_all(&settings, "PreToolUse");
    assert_eq!(pre_catch.len(), 1);

    // PostToolUse: unchanged, still exactly one in catch-all
    let post_catch = hooks_in_catch_all(&settings, "PostToolUse");
    assert_eq!(post_catch.len(), 1);
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
                "PostToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let diff = ClaudeCodeInstaller::uninstall_hooks_at(&path, false).unwrap();
    assert!(diff.is_some());

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let catch_all = hooks_in_catch_all(&settings, hook_type);
        assert!(
            !catch_all.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(is_git_ai_checkpoint_command)
                    .unwrap_or(false)
            }),
            "{hook_type}: git-ai should be removed"
        );
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
                "PreToolUse": [{"matcher": "Write|Edit|MultiEdit", "hooks": [
                    {"type":"command","command": "echo before"},
                    {"type":"command","command": cmd}
                ]}],
                "PostToolUse": [{"matcher": "Write|Edit|MultiEdit", "hooks": [
                    {"type":"command","command": "echo before"},
                    {"type":"command","command": cmd}
                ]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::uninstall_hooks_at(&path, false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let old_block = blocks
            .iter()
            .find(|b| b.get("matcher").and_then(|m| m.as_str()) == Some("Write|Edit|MultiEdit"))
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
fn u3_uninstall_from_multiple_blocks_preserves_user_hooks() {
    let (_td, path) = setup_test_env();
    let cmd = expected_cmd();
    let user = "echo user";
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [
                    {"matcher": "*", "hooks": [{"type":"command","command": cmd}, {"type":"command","command": user}]},
                    {"matcher": "Write|Edit|MultiEdit", "hooks": [{"type":"command","command": cmd}]}
                ],
                "PostToolUse": [
                    {"matcher": "*", "hooks": [{"type":"command","command": cmd}]},
                    {"matcher": "Write|Edit|MultiEdit", "hooks": [{"type":"command","command": cmd}, {"type":"command","command": user}]}
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    ClaudeCodeInstaller::uninstall_hooks_at(&path, false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["PreToolUse", "PostToolUse"] {
        let all_blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        // No git-ai anywhere
        let empty: Vec<Value> = Vec::new();
        for block in all_blocks {
            let hooks = block
                .get("hooks")
                .and_then(|h| h.as_array())
                .unwrap_or(&empty);
            assert!(!hooks.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(is_git_ai_checkpoint_command)
                    .unwrap_or(false)
            }));
        }
        // user hook still present somewhere
        let empty: Vec<Value> = Vec::new();
        let all_hooks: Vec<_> = all_blocks
            .iter()
            .flat_map(|b| {
                b.get("hooks")
                    .and_then(|h| h.as_array())
                    .unwrap_or(&empty)
                    .iter()
            })
            .collect();
        assert!(
            all_hooks
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some(user))
        );
    }
}

#[test]
fn u4_noop_uninstall_when_no_git_ai() {
    let (_td, path) = setup_test_env();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "hooks": {
                "PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": "echo hello"}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let diff = ClaudeCodeInstaller::uninstall_hooks_at(&path, false).unwrap();
    assert!(
        diff.is_none(),
        "should return None when nothing to uninstall"
    );
}

// ---- check_hooks scenarios ----

#[test]
fn c1_no_hooks_returns_not_installed() {
    let settings = json!({});
    let (installed, up_to_date) = ClaudeCodeInstaller::hook_status(&settings);
    assert!(!installed);
    assert!(!up_to_date);
}

#[test]
fn c2_git_ai_in_catch_all_returns_up_to_date() {
    let cmd = expected_cmd();
    let settings = json!({
        "hooks": {
            "PreToolUse": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}]}]
        }
    });
    let (installed, up_to_date) = ClaudeCodeInstaller::hook_status(&settings);
    assert!(installed);
    assert!(up_to_date);
}

#[test]
fn c3_git_ai_only_in_old_matcher_returns_installed_but_not_up_to_date() {
    let cmd = expected_cmd();
    let settings = json!({
        "hooks": {
            "PreToolUse": [{"matcher": "Write|Edit|MultiEdit", "hooks": [{"type":"command","command": cmd}]}]
        }
    });
    let (installed, up_to_date) = ClaudeCodeInstaller::hook_status(&settings);
    assert!(installed, "should be considered installed");
    assert!(!up_to_date, "should not be up-to-date when on old matcher");
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

/// Regression test for #1039: install_hooks_at should succeed even when
/// the parent directory does not yet exist.
#[test]
fn test_install_hooks_creates_missing_parent_dir() {
    let temp_dir = TempDir::new().unwrap();
    // Point to a settings.json inside a directory that does NOT exist yet
    let settings_path = temp_dir.path().join("missing_dir").join("settings.json");
    assert!(!settings_path.parent().unwrap().exists());

    let result = ClaudeCodeInstaller::install_hooks_at(&settings_path, &params(), false).unwrap();

    assert!(result.is_some(), "should report changes for fresh install");
    assert!(settings_path.exists(), "settings.json should be created");

    let content: Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).expect("valid JSON");
    let hooks = content.get("hooks").expect("hooks key should exist");
    assert!(hooks.get("PreToolUse").is_some());
    assert!(hooks.get("PostToolUse").is_some());
}
