use super::*;
use crate::operations::mdm::hook_installer::HookInstaller;
use crate::operations::mdm::test_env::with_temp_home;
use serial_test::serial;

fn test_binary_path() -> PathBuf {
    PathBuf::from("/tmp/git-ai/bin/git-ai")
}

#[test]
fn test_github_copilot_installer_name() {
    let installer = GitHubCopilotInstaller;
    assert_eq!(installer.name(), "GitHub Copilot");
}

#[test]
fn test_github_copilot_installer_id() {
    let installer = GitHubCopilotInstaller;
    assert_eq!(installer.id(), "github-copilot");
}

#[test]
#[serial]
fn test_install_hooks_creates_expected_file() {
    with_temp_home(|home| {
        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let diff = installer.install_hooks(&params, false).unwrap();
        assert!(diff.is_some());

        let hooks_path = home.join(".copilot").join("hooks").join("git-ai.json");
        assert!(hooks_path.exists());

        let content: Value =
            serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).expect("valid json");

        let pre = content
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .unwrap();
        let post = content
            .get("hooks")
            .and_then(|h| h.get("PostToolUse"))
            .and_then(|v| v.as_array())
            .unwrap();

        assert_eq!(pre.len(), 1);
        assert_eq!(post.len(), 1);
        assert_eq!(
            pre[0].get("command").and_then(|v| v.as_str()),
            Some("/tmp/git-ai/bin/git-ai checkpoint github-copilot --hook-input stdin")
        );
        assert_eq!(
            pre[0].get("powershell").and_then(|v| v.as_str()),
            Some("& '/tmp/git-ai/bin/git-ai' checkpoint github-copilot --hook-input stdin")
        );
    });
}

#[test]
#[serial]
fn test_install_hooks_quotes_windows_home_path_with_spaces_and_apostrophe() {
    with_temp_home(|home| {
        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: PathBuf::from(r"C:\Users\test user's\.git-ai\bin\git-ai.exe"),
        };

        installer.install_hooks(&params, false).unwrap();

        let hooks_path = home.join(".copilot").join("hooks").join("git-ai.json");
        let content: Value =
            serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).expect("valid json");
        let pre_hook = &content["hooks"]["PreToolUse"][0];

        assert_eq!(
            pre_hook.get("command").and_then(|v| v.as_str()),
            Some(
                "'C:/Users/test user'\\''s/.git-ai/bin/git-ai.exe' checkpoint github-copilot --hook-input stdin"
            )
        );
        assert_eq!(
            pre_hook.get("powershell").and_then(|v| v.as_str()),
            Some(
                "& 'C:/Users/test user''s/.git-ai/bin/git-ai.exe' checkpoint github-copilot --hook-input stdin"
            )
        );
    });
}

#[test]
#[serial]
fn test_install_hooks_upgrades_existing_windows_command_without_duplicates() {
    with_temp_home(|home| {
        let hooks_path = home.join(".copilot").join("hooks").join("git-ai.json");
        fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
        let existing = json!({
            "hooks": {
                "PreToolUse": [
                    {"type": "command", "command": "echo keep-me"},
                    {
                        "type": "command",
                        "matcher": "Edit|Write",
                        "timeoutSec": 15,
                        "command": "C:/Users/test user/.git-ai/bin/git-ai.exe checkpoint github-copilot --hook-input stdin"
                    }
                ],
                "PostToolUse": [{
                    "type": "command",
                    "matcher": "Edit|Write",
                    "timeoutSec": 15,
                    "command": "C:/Users/test user/.git-ai/bin/git-ai.exe checkpoint github-copilot --hook-input stdin"
                }]
            }
        });
        fs::write(
            &hooks_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: PathBuf::from(r"C:\Users\test user\.git-ai\bin\git-ai.exe"),
        };

        installer.install_hooks(&params, false).unwrap();

        let content: Value =
            serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).expect("valid json");
        let pre = content["hooks"]["PreToolUse"].as_array().unwrap();
        let post = content["hooks"]["PostToolUse"].as_array().unwrap();

        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["command"], "echo keep-me");
        assert_eq!(post.len(), 1);
        for hook in [&pre[1], &post[0]] {
            assert_eq!(hook["matcher"], "Edit|Write");
            assert_eq!(hook["timeoutSec"], 15);
            assert_eq!(
                hook["powershell"],
                "& 'C:/Users/test user/.git-ai/bin/git-ai.exe' checkpoint github-copilot --hook-input stdin"
            );
        }

        let status = installer.check_hooks(&params).unwrap();
        assert!(status.hooks_installed);
        assert!(status.hooks_up_to_date);
    });
}

#[test]
#[serial]
fn test_check_hooks_recognizes_quoted_windows_hook_as_up_to_date() {
    with_temp_home(|_| {
        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: PathBuf::from(r"C:\Users\test user\.git-ai\bin\git-ai.exe"),
        };

        installer.install_hooks(&params, false).unwrap();

        let status = installer.check_hooks(&params).unwrap();
        assert!(status.hooks_installed);
        assert!(status.hooks_up_to_date);
    });
}

#[test]
#[serial]
fn test_install_hooks_idempotent() {
    with_temp_home(|_| {
        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let first = installer.install_hooks(&params, false).unwrap();
        assert!(first.is_some());

        let second = installer.install_hooks(&params, false).unwrap();
        assert!(second.is_none());
    });
}

#[test]
#[serial]
fn test_install_hooks_deletes_legacy_hooks_file() {
    with_temp_home(|home| {
        let legacy_path = home.join(".github").join("hooks").join("git-ai.json");
        fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        fs::write(&legacy_path, r#"{"hooks":{}}"#).unwrap();
        assert!(legacy_path.exists());

        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        installer.install_hooks(&params, false).unwrap();

        assert!(!legacy_path.exists());
        let new_path = home.join(".copilot").join("hooks").join("git-ai.json");
        assert!(new_path.exists());
    });
}

#[test]
#[serial]
fn test_install_hooks_dry_run_does_not_create_files() {
    with_temp_home(|home| {
        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let hooks_dir = home.join(".copilot").join("hooks");
        let hooks_path = hooks_dir.join("git-ai.json");
        assert!(!hooks_dir.exists());
        assert!(!hooks_path.exists());

        let diff = installer.install_hooks(&params, true).unwrap();
        assert!(diff.is_some());
        assert!(!hooks_dir.exists());
        assert!(!hooks_path.exists());
    });
}

#[test]
#[serial]
fn test_install_hooks_repairs_non_object_hooks_field() {
    with_temp_home(|home| {
        let hooks_path = home.join(".copilot").join("hooks").join("git-ai.json");
        fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
        fs::write(&hooks_path, r#"{"hooks":"invalid","extra":"keep"}"#).unwrap();

        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let diff = installer.install_hooks(&params, false).unwrap();
        assert!(diff.is_some());

        let content: Value =
            serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).expect("valid json");
        assert_eq!(content.get("extra").and_then(|v| v.as_str()), Some("keep"));

        let pre = content
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .unwrap();
        let post = content
            .get("hooks")
            .and_then(|h| h.get("PostToolUse"))
            .and_then(|v| v.as_array())
            .unwrap();

        assert_eq!(pre.len(), 1);
        assert_eq!(post.len(), 1);
    });
}

#[test]
#[serial]
fn test_install_hooks_repairs_non_object_root() {
    with_temp_home(|home| {
        let hooks_path = home.join(".copilot").join("hooks").join("git-ai.json");
        fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
        fs::write(&hooks_path, "[]").unwrap();

        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let diff = installer.install_hooks(&params, false).unwrap();
        assert!(diff.is_some());

        let content: Value =
            serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).expect("valid json");
        let hooks = content.get("hooks").and_then(|v| v.as_object());
        assert!(hooks.is_some());
        assert!(hooks.unwrap().contains_key("PreToolUse"));
        assert!(hooks.unwrap().contains_key("PostToolUse"));
    });
}

#[test]
#[serial]
fn test_check_hooks_partial_pre_tool_use_counts_as_installed() {
    with_temp_home(|home| {
        let hooks_path = home.join(".copilot").join("hooks").join("git-ai.json");
        fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
        let existing = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "type": "command",
                        "command": "/tmp/git-ai/bin/git-ai checkpoint github-copilot --hook-input stdin"
                    }
                ]
            }
        });
        fs::write(
            &hooks_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let result = installer.check_hooks(&params).unwrap();
        assert!(result.tool_installed);
        assert!(result.hooks_installed);
        assert!(!result.hooks_up_to_date);
    });
}

#[test]
#[serial]
fn test_check_hooks_partial_post_tool_use_counts_as_installed() {
    with_temp_home(|home| {
        let hooks_path = home.join(".copilot").join("hooks").join("git-ai.json");
        fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
        let existing = json!({
            "hooks": {
                "PostToolUse": [
                    {
                        "type": "command",
                        "command": "/tmp/git-ai/bin/git-ai checkpoint github-copilot --hook-input stdin"
                    }
                ]
            }
        });
        fs::write(
            &hooks_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let result = installer.check_hooks(&params).unwrap();
        assert!(result.tool_installed);
        assert!(result.hooks_installed);
        assert!(!result.hooks_up_to_date);
    });
}

#[test]
#[serial]
fn test_uninstall_hooks_removes_only_git_ai_entries() {
    with_temp_home(|home| {
        let hooks_path = home.join(".copilot").join("hooks").join("git-ai.json");
        fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
        let existing = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "type": "command",
                        "command": "echo before"
                    },
                    {
                        "type": "command",
                        "command": "/tmp/git-ai/bin/git-ai checkpoint github-copilot --hook-input stdin"
                    }
                ],
                "PostToolUse": [
                    {
                        "type": "command",
                        "command": "/tmp/git-ai/bin/git-ai checkpoint github-copilot --hook-input stdin"
                    }
                ]
            }
        });
        fs::write(
            &hooks_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };
        let diff = installer.uninstall_hooks(&params, false).unwrap();
        assert!(diff.is_some());

        let content: Value =
            serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).expect("valid json");
        let pre = content
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .unwrap();
        let post = content
            .get("hooks")
            .and_then(|h| h.get("PostToolUse"))
            .and_then(|v| v.as_array())
            .unwrap();

        assert_eq!(pre.len(), 1);
        assert_eq!(
            pre[0].get("command").and_then(|v| v.as_str()),
            Some("echo before")
        );
        assert!(post.is_empty());
    });
}

#[test]
#[serial]
fn test_uninstall_hooks_deletes_legacy_hooks_file() {
    with_temp_home(|home| {
        let legacy_path = home.join(".github").join("hooks").join("git-ai.json");
        fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        fs::write(&legacy_path, r#"{"hooks":{}}"#).unwrap();

        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        installer.uninstall_hooks(&params, false).unwrap();
        assert!(!legacy_path.exists());
    });
}

#[test]
#[serial]
fn test_check_hooks_detects_legacy_path_as_installed() {
    with_temp_home(|home| {
        let legacy_path = home.join(".github").join("hooks").join("git-ai.json");
        fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        let existing = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "type": "command",
                        "command": "/tmp/git-ai/bin/git-ai checkpoint github-copilot --hook-input stdin"
                    }
                ]
            }
        });
        fs::write(
            &legacy_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        let installer = GitHubCopilotInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        let result = installer.check_hooks(&params).unwrap();
        assert!(result.tool_installed);
        assert!(result.hooks_installed);
        assert!(!result.hooks_up_to_date);
    });
}
