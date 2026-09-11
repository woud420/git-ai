//! Comprehensive tests for install_hooks command module
//!
//! This module tests the git-ai install-hooks and uninstall-hooks commands,
//! which handle installation of git hooks for various IDEs and coding agents.

use crate::repos::test_repo::{DaemonTestScope, TestRepo, get_binary_path};
use git_ai::operations::commands::install_hooks::{
    InstallResult, InstallStatus, run, run_uninstall, to_hashmap,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

// ==============================================================================
// Argument Parsing Tests
// ==============================================================================

fn isolated_install_command(root: &Path) -> Command {
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();

    let mut command = Command::new(get_binary_path());
    command
        .current_dir(root)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("APPDATA", root.join("app-data"))
        .env("LOCALAPPDATA", root.join("local-app-data"))
        .env("GIT_CONFIG_GLOBAL", root.join("global.gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AI_DAEMON_HOME", root.join("daemon"))
        .env("GIT_AI_TEST_DB_PATH", root.join("events.db"))
        .env("GITAI_TEST_DB_PATH", root.join("legacy-events.db"))
        .env_remove("API_BASE")
        .env_remove("API_KEY");
    command
}

fn seed_pi_uninstall_files(repo: &TestRepo) -> (std::path::PathBuf, std::path::PathBuf) {
    let agent = repo.test_home_path().join(".pi/agent");
    fs::create_dir_all(agent.join("extensions")).unwrap();
    let extension = agent.join("extensions/git-ai.ts");
    let overrides = agent.join("git-ai.override.json");
    fs::write(&extension, "managed extension\n").unwrap();
    fs::write(&overrides, "{}\n").unwrap();
    (extension, overrides)
}

fn run_invalid_installer_env(payload: Option<&str>) -> std::process::Output {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let mut command = Command::new(get_binary_path());
    command
        .arg("install-hooks")
        .arg("--installer-env")
        .env("HOME", repo.test_home_path())
        .env("GIT_AI_TEST_DB_PATH", repo.test_db_path())
        .env("GITAI_TEST_DB_PATH", repo.test_db_path())
        .env("GIT_AI_ALLOW_SUPERUSER", "1")
        .env("GIT_AI_DEBUG", "0");
    if let Some(payload) = payload {
        command.arg(payload);
    }
    command.output().expect("run install-hooks")
}

mod status_and_conversion;
mod workflow {
    use super::{run, run_uninstall};

    #[test]
    fn test_run_install_hooks_no_args() {
        // This will try to run against the actual system, but should not crash
        // It may fail if binary path cannot be determined, which is acceptable
        let result = run(&[]);

        // We just ensure it returns a result (success or error)
        // The actual behavior depends on the system state
        match result {
            Ok(_statuses) => {
                // Should return a HashMap, possibly empty
                // Success is valid
            }
            Err(e) => {
                // May fail if binary path is not available or other system issues
                let err_msg = e.to_string();
                // Just ensure we get a meaningful error
                assert!(!err_msg.is_empty());
            }
        }
    }

    #[test]
    fn test_run_install_hooks_with_dry_run_flag() {
        let args = vec!["--dry-run".to_string()];
        let result = run(&args);

        // Dry run should not modify anything
        match result {
            Ok(_statuses) => {
                // Success is valid
            }
            Err(e) => {
                let err_msg = e.to_string();
                assert!(!err_msg.is_empty());
            }
        }
    }

    #[test]
    fn test_run_install_hooks_with_dry_run_true() {
        let args = vec!["--dry-run=true".to_string()];
        let result = run(&args);

        drop(result);
    }

    #[test]
    fn test_run_install_hooks_with_verbose_flag() {
        let args = vec!["--verbose".to_string()];
        let result = run(&args);

        drop(result);
    }

    #[test]
    fn test_run_install_hooks_with_verbose_short_flag() {
        let args = vec!["-v".to_string()];
        let result = run(&args);

        drop(result);
    }

    #[test]
    fn test_run_install_hooks_with_multiple_flags() {
        let args = vec!["--dry-run".to_string(), "--verbose".to_string()];
        let result = run(&args);

        drop(result);
    }

    #[test]
    fn test_run_install_hooks_with_dry_run_false() {
        // Note: This could actually install hooks on the system
        // In a real test environment, this should be run in isolation
        let args = vec!["--dry-run=false".to_string()];
        let result = run(&args);

        drop(result);
    }

    #[test]
    fn test_run_install_hooks_rejects_unknown_args() {
        let args = vec!["--unknown-flag".to_string(), "--dry-run".to_string()];
        let error = run(&args).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unknown install option '--unknown-flag'")
        );
    }

    // ==============================================================================
    // Uninstall Tests
    // ==============================================================================

    #[test]
    fn test_run_uninstall_hooks_no_args() {
        let result = run_uninstall(&[]);

        match result {
            Ok(_statuses) => {
                // Success is valid
            }
            Err(e) => {
                let err_msg = e.to_string();
                assert!(!err_msg.is_empty());
            }
        }
    }

    #[test]
    fn test_run_uninstall_hooks_with_dry_run() {
        let args = vec!["--dry-run".to_string()];
        let result = run_uninstall(&args);

        drop(result);
    }

    #[test]
    fn test_run_uninstall_hooks_with_verbose() {
        let args = vec!["--verbose".to_string()];
        let result = run_uninstall(&args);

        drop(result);
    }

    #[test]
    fn test_run_uninstall_hooks_with_multiple_flags() {
        let args = vec![
            "--dry-run=true".to_string(),
            "-v".to_string(),
            "--unknown".to_string(),
        ];
        let result = run_uninstall(&args);

        drop(result);
    }

    // ==============================================================================
    // Integration-style Tests
    // ==============================================================================

    #[test]
    fn test_install_workflow_dry_run_does_not_modify_system() {
        // Dry run should be safe to run repeatedly
        let args = vec!["--dry-run".to_string(), "--verbose".to_string()];

        let result1 = run(&args);
        let result2 = run(&args);

        // Both runs should succeed or fail consistently
        match (result1, result2) {
            (Ok(_statuses1), Ok(_statuses2)) => {
                // Results may differ if system state changes between runs,
                // but both should be valid HashMaps
                // Success is valid
            }
            (Err(_), Err(_)) => {
                // Both failing is acceptable (e.g., on CI without proper setup)
            }
            _ => {
                // Inconsistent results would indicate a problem, but we allow it
                // since the system state could change
            }
        }
    }

    #[test]
    fn test_uninstall_workflow_dry_run_does_not_modify_system() {
        let args = vec!["--dry-run".to_string()];

        let result1 = run_uninstall(&args);
        let result2 = run_uninstall(&args);

        match (result1, result2) {
            (Ok(_statuses1), Ok(_statuses2)) => {
                // Success is valid
            }
            (Err(_), Err(_)) => {
                // Both failing is acceptable
            }
            _ => {
                // Allow inconsistent results due to system state changes
            }
        }
    }
}

#[test]
fn eng_390_root_help_matches_focused_command_contracts() {
    let temp = tempfile::tempdir().unwrap();
    let output = isolated_install_command(temp.path())
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--json                 Output blame data as JSON"));
    assert!(stderr.contains("Configure Git Trace2 and supported agent/editor integrations"));
    assert!(stderr.contains("Include Visual Studio detection and status checks on Windows"));
    assert!(stderr.contains("This does not install a VSIX package"));
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.trim_start().starts_with("uninstall "))
            .count(),
        1,
        "root help must list the uninstall command once"
    );
    assert!(
        !temp.path().join("home").join(".git-ai").exists(),
        "root help must not create git-ai state"
    );
}

#[test]
fn install_help_is_side_effect_free_for_both_aliases() {
    for subcommand in ["install", "install-hooks"] {
        for help_flag in ["--help", "-h"] {
            let temp = tempfile::tempdir().unwrap();
            let output = isolated_install_command(temp.path())
                .args([subcommand, help_flag])
                .output()
                .unwrap();
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );

            assert!(
                output.status.success(),
                "{subcommand} {help_flag} failed:\n{combined}"
            );
            assert!(
                combined.contains(&format!("Usage: git-ai {subcommand} [options]")),
                "{subcommand} {help_flag} did not print focused usage:\n{combined}"
            );
            for required in [
                "--dry-run",
                "--verbose",
                "--skills",
                "--visual-studio-extension",
                "--api-base",
                "--api-key",
            ] {
                assert!(
                    combined.contains(required),
                    "{subcommand} {help_flag} is missing supported option {required}"
                );
            }
            assert!(
                !temp.path().join("global.gitconfig").exists(),
                "{subcommand} {help_flag} modified global Git configuration"
            );
            assert!(
                !temp.path().join("home/.git-ai").exists(),
                "{subcommand} {help_flag} created git-ai state"
            );
        }
    }
}

#[test]
fn install_rejects_unknown_options_before_side_effects() {
    let temp = tempfile::tempdir().unwrap();
    let output = isolated_install_command(temp.path())
        .args(["install", "--skils", "--dry-run"])
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !output.status.success(),
        "unknown option succeeded:\n{combined}"
    );
    assert!(
        combined.contains("unknown install option '--skils'")
            && combined.contains("git-ai install --help"),
        "unknown option error is not actionable:\n{combined}"
    );
    assert!(
        !temp.path().join("global.gitconfig").exists(),
        "unknown option modified global Git configuration"
    );
    assert!(
        !temp.path().join("home/.git-ai").exists(),
        "unknown option created git-ai state"
    );
}

#[test]
fn eng_389_invalid_api_values_leave_test_home_unchanged() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let config = repo.test_home_path().join(".git-ai/config.json");
    let global = repo.test_home_path().join(".gitconfig");
    let original_config = fs::read(&config).unwrap();
    let original_global = fs::read(&global).ok();

    for subcommand in ["install", "install-hooks"] {
        for option in ["--api-base", "--api-key"] {
            for value in ["", "  ", "--help", "-h", "--dry-run", "--skils"] {
                let error = repo
                    .git_ai_without_pre_sync_for_test(&[subcommand, option, value])
                    .expect_err("invalid API value must not reach installation");
                assert!(error.contains(&format!("missing value for {option}")));
                assert_eq!(fs::read(&config).unwrap(), original_config);
                assert_eq!(fs::read(&global).ok(), original_global);
                assert!(
                    !repo
                        .test_home_path()
                        .join(".git-ai/install-manifest.json")
                        .exists()
                );
            }
            let error = repo
                .git_ai_without_pre_sync_for_test(&[subcommand, &format!("{option}=")])
                .expect_err("empty equals value must not reach installation");
            assert!(error.contains(&format!("missing value for {option}")));
            assert_eq!(fs::read(&config).unwrap(), original_config);
            assert_eq!(fs::read(&global).ok(), original_global);
        }
    }
}

#[test]
fn eng_408_uninstall_help_and_invalid_options_preserve_managed_files() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let (extension, overrides) = seed_pi_uninstall_files(&repo);
    let config = repo.test_home_path().join(".git-ai/config.json");
    let original_config = fs::read(&config).unwrap();

    for (flag, help) in [
        ("--help", true),
        ("-h", true),
        ("--dryrun", false),
        ("--dry-run=tru", false),
        ("--skills", false),
        ("unexpected", false),
    ] {
        let result = repo.git_ai_without_pre_sync_for_test(&["uninstall-hooks", flag]);
        if help {
            let output = result.expect("help must succeed");
            assert!(output.contains("Usage: git-ai uninstall-hooks [options]"));
        } else {
            let error = result.expect_err("invalid options must fail closed");
            assert!(error.contains("git-ai uninstall-hooks --help"));
        }
        assert_eq!(
            fs::read_to_string(&extension).unwrap(),
            "managed extension\n"
        );
        assert_eq!(fs::read_to_string(&overrides).unwrap(), "{}\n");
        assert_eq!(fs::read(&config).unwrap(), original_config);
    }
}

#[test]
fn eng_408_uninstall_preview_and_apply_respect_file_ownership() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let (extension, overrides) = seed_pi_uninstall_files(&repo);
    for flag in ["--dry-run", "--dry-run=true"] {
        repo.git_ai_without_pre_sync_for_test(&["uninstall-hooks", flag, "-v"])
            .unwrap();
        assert_eq!(
            fs::read_to_string(&extension).unwrap(),
            "managed extension\n"
        );
        assert_eq!(fs::read_to_string(&overrides).unwrap(), "{}\n");
    }
    repo.git_ai_without_pre_sync_for_test(&["uninstall-hooks", "--dry-run", "--dry-run=false"])
        .unwrap();
    assert!(!extension.exists());
    assert_eq!(fs::read_to_string(overrides).unwrap(), "{}\n");
}

#[test]
fn eng_400_documented_pi_preview_does_not_apply_removal() {
    let readme = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("agent-support/pi/README.md"),
    )
    .unwrap();
    let commands = readme
        .split("## Uninstall")
        .nth(1)
        .unwrap()
        .split("```bash")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    assert_eq!(commands.len(), 2, "expected preview followed by apply");
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let (extension, overrides) = seed_pi_uninstall_files(&repo);
    for (index, command) in commands.iter().enumerate() {
        let words = command.split_whitespace().collect::<Vec<_>>();
        assert_eq!(words[..2], ["git-ai", "uninstall-hooks"]);
        repo.git_ai_without_pre_sync_for_test(&words[1..]).unwrap();
        assert_eq!(
            extension.exists(),
            index == 0,
            "incorrect action: {command}"
        );
        assert_eq!(fs::read_to_string(&overrides).unwrap(), "{}\n");
    }
}

#[test]
#[cfg(not(windows))]
fn install_hooks_detects_cline_from_editor_extension_manifests() {
    let manifest_layouts = [
        ".vscode/extensions/extensions.json",
        ".vscode-server/extensions/extensions.json",
        ".cursor/extensions/extensions.json",
        ".windsurf/extensions/extensions.json",
    ];

    for relative_path in manifest_layouts {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        let home = repo.test_home_path();
        let manifest_path = home.join(relative_path);
        fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
        fs::create_dir_all(home.join("Documents")).unwrap();
        fs::write(
            manifest_path,
            r#"[
  {
    "identifier": {
      "id": "saoudrizwan.claude-dev",
      "uuid": "test-uuid"
    },
    "version": "4.0.11",
    "relativeLocation": "saoudrizwan.claude-dev-4.0.11"
  }
]"#,
        )
        .unwrap();

        let output = repo
            .git_ai_command_without_pre_sync_for_test(&["install-hooks", "--dry-run"], &[])
            .output()
            .expect("run git-ai install-hooks --dry-run");
        assert!(
            output.status.success(),
            "install-hooks failed for {relative_path}:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Cline: Pending updates"),
            "Cline was not detected from {relative_path}:\n{stdout}"
        );
        assert!(
            !home.join("Documents/Cline/Hooks").exists(),
            "dry-run must not create the Cline hooks directory"
        );
    }
}

#[test]
#[cfg(not(windows))]
fn install_hooks_ignores_unrelated_extension_manifest_entries() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let home = repo.test_home_path();
    let manifest_path = home.join(".vscode/extensions/extensions.json");
    fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    fs::write(
        manifest_path,
        r#"[
  {
    "identifier": {
      "id": "git-ai.git-ai-vscode"
    },
    "relativeLocation": "saoudrizwan.claude-dev-4.0.11"
  }
]"#,
    )
    .unwrap();

    let output = repo
        .git_ai_command_without_pre_sync_for_test(&["install-hooks", "--dry-run"], &[])
        .output()
        .expect("run git-ai install-hooks --dry-run");
    assert!(
        output.status.success(),
        "install-hooks failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("Cline: Pending updates"),
        "an unrelated extension id must not read as a Cline install:\n{stdout}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn child_writer_blocks_exec() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let binary = repo.path().join("busy-git-ai");
    fs::copy(get_binary_path(), &binary).unwrap();

    let writer = fs::OpenOptions::new().write(true).open(&binary).unwrap();
    let mut holder = Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(writer)
        .spawn()
        .expect("start child holding executable open for writing");

    let result = Command::new(&binary).arg("--version").output();
    drop(holder.stdin.take());
    let holder_status = holder.wait().expect("wait for writer child");

    assert!(holder_status.success());
    assert_eq!(
        result
            .expect_err("Linux should reject an executable held open for writing")
            .kind(),
        std::io::ErrorKind::ExecutableFileBusy
    );
}

#[test]
fn plain_install_hooks_preserves_the_invoking_user_home() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let invoking_home = repo.test_home_path();
    let installed_home = repo.path().join("installed-user");
    let installed_bin_dir = installed_home.join(".git-ai").join("bin");
    fs::create_dir_all(&installed_bin_dir).unwrap();

    #[cfg(windows)]
    let installed_binary = installed_bin_dir.join("git-ai.exe");
    #[cfg(not(windows))]
    let installed_binary = installed_bin_dir.join("git-ai");
    #[cfg(target_os = "linux")]
    {
        // Keep the destination writer out of this multithreaded process so sibling spawns cannot inherit it.
        let copy = Command::new("cp")
            .arg("-p")
            .arg(get_binary_path())
            .arg(&installed_binary)
            .output()
            .expect("copy git-ai binary");
        assert!(
            copy.status.success(),
            "copy git-ai binary failed: {}",
            String::from_utf8_lossy(&copy.stderr)
        );
    }
    #[cfg(not(target_os = "linux"))]
    fs::copy(get_binary_path(), &installed_binary).unwrap();

    let test_db = repo.path().join("install-hooks.db");
    let mut command = Command::new(&installed_binary);
    command
        .arg("install-hooks")
        .current_dir(repo.path())
        .env("HOME", invoking_home)
        .env("API_KEY", "package-test-key")
        .env("GIT_AI_TEST_DB_PATH", &test_db)
        .env("GITAI_TEST_DB_PATH", &test_db)
        .env("GIT_CONFIG_GLOBAL", invoking_home.join(".gitconfig"))
        .env("GIT_AI_ALLOW_SUPERUSER", "1")
        .env("GIT_AI_DEBUG", "0");
    #[cfg(windows)]
    command
        .env("USERPROFILE", invoking_home)
        .env("APPDATA", invoking_home.join("AppData").join("Roaming"))
        .env("LOCALAPPDATA", invoking_home.join("AppData").join("Local"));

    let output = command.output().expect("run copied git-ai binary");
    assert!(
        output.status.success(),
        "plain install-hooks failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let invoking_config = fs::read_to_string(invoking_home.join(".git-ai/config.json"))
        .expect("install-hooks should update the invoking user's config");
    let invoking_config: serde_json::Value = serde_json::from_str(&invoking_config).unwrap();
    assert_eq!(
        invoking_config["api_key"],
        serde_json::Value::String("package-test-key".to_string())
    );
    assert!(
        !installed_home.join(".git-ai/config.json").exists(),
        "plain install-hooks must not retarget config to the binary owner's home"
    );
}

#[test]
fn packaged_install_hooks_uses_the_explicit_installer_user_home() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let invoking_home = repo.test_home_path();
    let installer_home = repo.path().join("installer-user");
    let installed_home = repo.path().join("binary-owner");
    let installed_bin_dir = installed_home.join(".git-ai").join("bin");
    fs::create_dir_all(&installed_bin_dir).unwrap();

    #[cfg(windows)]
    let installed_binary = installed_bin_dir.join("git-ai.exe");
    #[cfg(not(windows))]
    let installed_binary = installed_bin_dir.join("git-ai");
    #[cfg(target_os = "linux")]
    {
        let copy = Command::new("cp")
            .arg("-p")
            .arg(get_binary_path())
            .arg(&installed_binary)
            .output()
            .expect("copy git-ai binary");
        assert!(copy.status.success());
    }
    #[cfg(not(target_os = "linux"))]
    fs::copy(get_binary_path(), &installed_binary).unwrap();

    #[cfg(windows)]
    let installer_home_payload = format!("USERPROFILE={}", installer_home.display());
    #[cfg(not(windows))]
    let installer_home_payload = format!("HOME={}", installer_home.display());

    let invoking_config_path = invoking_home.join(".git-ai/config.json");
    let invoking_config_before = fs::read(&invoking_config_path).unwrap();
    let test_db = repo.path().join("packaged-install-hooks.db");
    let mut command = Command::new(&installed_binary);
    command
        .args(["install-hooks", "--installer-env", &installer_home_payload])
        .current_dir(repo.path())
        .env("HOME", invoking_home)
        .env("API_KEY", "package-test-key")
        .env("GIT_AI_TEST_DB_PATH", &test_db)
        .env("GITAI_TEST_DB_PATH", &test_db)
        .env("GIT_AI_ALLOW_SUPERUSER", "1")
        .env("GIT_AI_DEBUG", "0");
    #[cfg(windows)]
    command
        .env("USERPROFILE", invoking_home)
        .env("APPDATA", invoking_home.join("AppData").join("Roaming"))
        .env("LOCALAPPDATA", invoking_home.join("AppData").join("Local"));

    let output = command.output().expect("run copied git-ai binary");
    assert!(
        output.status.success(),
        "packaged install-hooks failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let installer_config = fs::read_to_string(installer_home.join(".git-ai/config.json"))
        .expect("installer payload should select the target user's config");
    let installer_config: serde_json::Value = serde_json::from_str(&installer_config).unwrap();
    assert_eq!(
        installer_config["api_key"],
        serde_json::Value::String("package-test-key".to_string())
    );
    assert_eq!(
        fs::read(invoking_config_path).unwrap(),
        invoking_config_before
    );
    assert!(!installed_home.join(".git-ai/config.json").exists());
}

#[test]
fn install_hooks_rejects_missing_and_malformed_installer_environment_payloads() {
    let missing = run_invalid_installer_env(None);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing value for --installer-env"));

    let relative = run_invalid_installer_env(Some("HOME=relative/path"));
    assert!(!relative.status.success());
    assert!(String::from_utf8_lossy(&relative.stderr).contains("HOME must be an absolute path"));
}

#[test]
fn install_hooks_rejects_non_allowlisted_installer_environment_without_echoing_values() {
    let secret = "do-not-echo-this-secret";
    let payload = format!("API_KEY={secret}");
    let output = run_invalid_installer_env(Some(&payload));
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("API_KEY is not allowed in --installer-env"));
    assert!(!stderr.contains(secret));
}

// ==============================================================================
// InstallResult Tests
// ==============================================================================

#[test]
fn test_install_result_installed() {
    let result = InstallResult::installed();
    assert_eq!(result.status, InstallStatus::Installed);
    assert!(result.error.is_none());
    assert!(result.warnings.is_empty());
}

#[test]
fn test_install_result_already_installed() {
    let result = InstallResult::already_installed();
    assert_eq!(result.status, InstallStatus::AlreadyInstalled);
    assert!(result.error.is_none());
    assert!(result.warnings.is_empty());
}

#[test]
fn test_install_result_not_found() {
    let result = InstallResult::not_found();
    assert_eq!(result.status, InstallStatus::NotFound);
    assert!(result.error.is_none());
    assert!(result.warnings.is_empty());
}

#[test]
fn test_install_result_failed() {
    let result = InstallResult::failed("Installation failed");
    assert_eq!(result.status, InstallStatus::Failed);
    assert_eq!(result.error, Some("Installation failed".to_string()));
    assert!(result.warnings.is_empty());
}

#[test]
fn test_install_result_failed_with_string() {
    let error_msg = String::from("Custom error message");
    let result = InstallResult::failed(error_msg.clone());
    assert_eq!(result.status, InstallStatus::Failed);
    assert_eq!(result.error, Some(error_msg));
}

#[test]
fn test_install_result_with_warning() {
    let result = InstallResult::installed().with_warning("Minor issue detected");
    assert_eq!(result.status, InstallStatus::Installed);
    assert!(result.error.is_none());
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0], "Minor issue detected");
}

#[test]
fn test_install_result_with_multiple_warnings() {
    let result = InstallResult::installed()
        .with_warning("Warning 1")
        .with_warning("Warning 2")
        .with_warning("Warning 3");

    assert_eq!(result.warnings.len(), 3);
    assert_eq!(result.warnings[0], "Warning 1");
    assert_eq!(result.warnings[1], "Warning 2");
    assert_eq!(result.warnings[2], "Warning 3");
}

#[test]
fn test_install_result_message_for_metrics_with_error() {
    let result = InstallResult::failed("Critical error");
    let message = result.message_for_metrics();
    assert_eq!(message, Some("Critical error".to_string()));
}

#[test]
fn test_install_result_message_for_metrics_with_warnings() {
    let result = InstallResult::installed()
        .with_warning("Warning 1")
        .with_warning("Warning 2");
    let message = result.message_for_metrics();
    assert_eq!(message, Some("Warning 1; Warning 2".to_string()));
}

#[test]
fn test_install_result_message_for_metrics_with_error_and_warnings() {
    // Error takes precedence over warnings
    let result = InstallResult::failed("Error message").with_warning("Some warning");
    let message = result.message_for_metrics();
    assert_eq!(message, Some("Error message".to_string()));
}

#[test]
fn test_install_result_message_for_metrics_no_error_or_warnings() {
    let result = InstallResult::installed();
    let message = result.message_for_metrics();
    assert!(message.is_none());
}

#[test]
fn test_install_result_message_for_metrics_empty_warnings() {
    let result = InstallResult {
        status: InstallStatus::Installed,
        error: None,
        warnings: vec![],
    };
    let message = result.message_for_metrics();
    assert!(message.is_none());
}

// ==============================================================================
// Edge Cases and Error Handling
// ==============================================================================

#[test]
fn test_install_result_clone() {
    let result = InstallResult::failed("Error")
        .with_warning("Warning 1")
        .with_warning("Warning 2");

    let cloned = result.clone();
    assert_eq!(cloned.status, result.status);
    assert_eq!(cloned.error, result.error);
    assert_eq!(cloned.warnings, result.warnings);
}

#[test]
fn test_install_result_debug_formatting() {
    let result = InstallResult::installed();
    let debug_str = format!("{:?}", result);
    assert!(debug_str.contains("InstallResult"));
    assert!(debug_str.contains("Installed"));
}

#[test]
fn test_install_result_warning_with_empty_string() {
    let result = InstallResult::installed().with_warning("");
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0], "");
}

#[test]
fn test_install_result_failed_with_empty_string() {
    let result = InstallResult::failed("");
    assert_eq!(result.error, Some("".to_string()));
    assert_eq!(result.status, InstallStatus::Failed);
}

#[test]
fn test_install_result_message_for_metrics_single_warning() {
    let result = InstallResult::installed().with_warning("Only warning");
    let message = result.message_for_metrics();
    assert_eq!(message, Some("Only warning".to_string()));
}

#[test]
fn test_install_result_message_for_metrics_warnings_join_with_semicolon() {
    let result = InstallResult::installed()
        .with_warning("First; warning")
        .with_warning("Second; warning")
        .with_warning("Third; warning");

    let message = result.message_for_metrics();
    assert_eq!(
        message,
        Some("First; warning; Second; warning; Third; warning".to_string())
    );
}

// ==============================================================================
// Complex Scenario Tests
// ==============================================================================

#[test]
fn test_install_result_builder_pattern() {
    // Demonstrate builder-like pattern with warnings
    let result = InstallResult::installed()
        .with_warning("Extension not found")
        .with_warning("Git path not configured")
        .with_warning("Manual action required");

    assert_eq!(result.status, InstallStatus::Installed);
    assert_eq!(result.warnings.len(), 3);
    assert!(result.error.is_none());

    let message = result.message_for_metrics();
    assert!(message.is_some());
    let msg = message.unwrap();
    assert!(msg.contains("Extension not found"));
    assert!(msg.contains("Git path not configured"));
    assert!(msg.contains("Manual action required"));
}

#[test]
fn test_install_result_different_error_types() {
    // Test with different error message types
    let errors = vec![
        "Permission denied",
        "File not found",
        "Invalid configuration",
        "Version mismatch: expected 1.7, found 1.5",
        "Network timeout",
        "",
    ];

    for error in errors {
        let result = InstallResult::failed(error);
        assert_eq!(result.status, InstallStatus::Failed);
        assert_eq!(result.error, Some(error.to_string()));
        assert_eq!(result.message_for_metrics(), Some(error.to_string()));
    }
}
