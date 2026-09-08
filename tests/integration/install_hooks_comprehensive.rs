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
mod workflow;
