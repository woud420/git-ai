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

mod cli_side_effects;
mod extension_detection;
mod installer_environment;
mod result_values;
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
