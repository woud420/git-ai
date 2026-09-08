use super::*;

#[test]
fn u1_uninstall_from_catch_all() {
    let (_td, path) = setup_test_env();
    let bc = expected_before_cmd();
    let ac = expected_after_cmd();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "tools": {"enableHooks": true},
            "hooks": {
                "BeforeTool": [{"matcher": "*", "hooks": [{"type":"command","command": bc}]}],
                "AfterTool": [{"matcher": "*", "hooks": [{"type":"command","command": ac}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let diff = GeminiInstaller::uninstall_hooks_at(&path, false).unwrap();
    assert!(diff.is_some());

    let settings = read_settings(&path);
    for hook_type in &["BeforeTool", "AfterTool"] {
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
    let bc = expected_before_cmd();
    let ac = expected_after_cmd();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "tools": {"enableHooks": true},
            "hooks": {
                "BeforeTool": [{"matcher": "write_file|replace", "hooks": [{"type":"command","command": "echo before"}, {"type":"command","command": bc}]}],
                "AfterTool": [{"matcher": "write_file|replace", "hooks": [{"type":"command","command": "echo after"}, {"type":"command","command": ac}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    GeminiInstaller::uninstall_hooks_at(&path, false).unwrap();

    let settings = read_settings(&path);
    for (hook_type, user_cmd) in &[("BeforeTool", "echo before"), ("AfterTool", "echo after")] {
        let blocks = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|v| v.as_array())
            .unwrap();
        let old_block = blocks
            .iter()
            .find(|b| b.get("matcher").and_then(|m| m.as_str()) == Some("write_file|replace"))
            .unwrap();
        let hooks = old_block.get("hooks").and_then(|h| h.as_array()).unwrap();
        assert!(
            hooks
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some(*user_cmd))
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
    let bc = expected_before_cmd();
    let ac = expected_after_cmd();
    let user = "echo user";
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "tools": {"enableHooks": true},
            "hooks": {
                "BeforeTool": [
                    {"matcher": "*", "hooks": [{"type":"command","command": bc}, {"type":"command","command": user}]},
                    {"matcher": "write_file|replace", "hooks": [{"type":"command","command": bc}]}
                ],
                "AfterTool": [
                    {"matcher": "*", "hooks": [{"type":"command","command": ac}]},
                    {"matcher": "write_file|replace", "hooks": [{"type":"command","command": ac}, {"type":"command","command": user}]}
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    GeminiInstaller::uninstall_hooks_at(&path, false).unwrap();

    let settings = read_settings(&path);
    for hook_type in &["BeforeTool", "AfterTool"] {
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
        serde_json::to_string_pretty(&json!({"hooks": {"BeforeTool": [{"matcher": "*", "hooks": [{"type":"command","command": "echo hello"}]}]}}))
            .unwrap(),
    )
    .unwrap();

    let diff = GeminiInstaller::uninstall_hooks_at(&path, false).unwrap();
    assert!(diff.is_none());
}

// ---- check_hooks scenarios ----

#[test]
fn c1_no_hooks_returns_not_installed() {
    let (installed, up_to_date) = GeminiInstaller::hook_status(&json!({}));
    assert!(!installed);
    assert!(!up_to_date);
}

#[test]
fn c2_git_ai_in_catch_all_returns_up_to_date() {
    let cmd = expected_before_cmd();
    let settings = json!({"hooks": {"BeforeTool": [{"matcher": "*", "hooks": [{"type":"command","command": cmd}]}]}});
    let (installed, up_to_date) = GeminiInstaller::hook_status(&settings);
    assert!(installed);
    assert!(up_to_date);
}

#[test]
fn c3_git_ai_only_in_old_matcher_not_up_to_date() {
    let cmd = expected_before_cmd();
    let settings = json!({"hooks": {"BeforeTool": [{"matcher": "write_file|replace", "hooks": [{"type":"command","command": cmd}]}]}});
    let (installed, up_to_date) = GeminiInstaller::hook_status(&settings);
    assert!(installed);
    assert!(!up_to_date);
}
