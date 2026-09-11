use super::*;
use serial_test::serial;
use std::path::PathBuf;
use tempfile::tempdir;

struct EnvVarGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        // SAFETY: tests marked `serial` avoid concurrent env mutation.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, old }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: tests marked `serial` avoid concurrent env mutation.
        unsafe {
            if let Some(old) = &self.old {
                std::env::set_var(self.key, old);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

fn test_binary_path(install_dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        install_dir.join("git-ai.exe")
    }

    #[cfg(not(windows))]
    {
        install_dir.join("git-ai")
    }
}

fn write_install_git_marker(install_dir: &Path, git_path: &str) {
    #[cfg(windows)]
    {
        fs::write(
            install_dir.join("git-og.cmd"),
            format!("@echo off\r\n\"{}\" %*\r\n", git_path),
        )
        .unwrap();
    }

    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(git_path, install_dir.join("git-og")).unwrap();
    }
}

#[test]
#[cfg(not(windows))]
#[serial]
fn install_reports_cline_hook_check_errors_as_failed() {
    let temp = tempdir().unwrap();
    let storage = temp.path().join("cline-storage");
    fs::create_dir_all(&storage).unwrap();
    fs::write(temp.path().join("Documents"), "not a directory").unwrap();
    let _home = EnvVarGuard::set("HOME", temp.path().to_str().unwrap());
    let _cline_storage = EnvVarGuard::set("GIT_AI_CLINE_STORAGE_PATH", storage.to_str().unwrap());
    let statuses = run(&["--dry-run".to_string()]).unwrap();
    assert_eq!(statuses.get("cline").map(String::as_str), Some("failed"));
}

#[test]
#[cfg(not(windows))]
#[serial]
fn uninstall_reports_cline_hook_check_errors_as_failed() {
    let temp = tempdir().unwrap();
    let storage = temp.path().join("cline-storage");
    fs::create_dir_all(&storage).unwrap();
    fs::write(temp.path().join("Documents"), "not a directory").unwrap();
    let _home = EnvVarGuard::set("HOME", temp.path().to_str().unwrap());
    let _cline_storage = EnvVarGuard::set("GIT_AI_CLINE_STORAGE_PATH", storage.to_str().unwrap());
    let statuses = run_uninstall(&["--dry-run".to_string()]).unwrap();
    assert_eq!(statuses.get("cline").map(String::as_str), Some("failed"));
}

#[test]
#[serial]
fn persist_install_config_updates_api_base_and_backfills_git_path() {
    let temp = tempdir().unwrap();
    let install_dir = temp.path().join("bin");
    fs::create_dir_all(&install_dir).unwrap();
    fs::write(test_binary_path(&install_dir), "").unwrap();
    let expected_git_path = if cfg!(windows) {
        r"C:\Program Files\Git\bin\git.exe"
    } else {
        "/opt/custom/bin/git"
    };
    write_install_git_marker(&install_dir, expected_git_path);
    let _home = EnvVarGuard::set("HOME", temp.path().to_str().unwrap());
    #[cfg(windows)]
    let _userprofile = EnvVarGuard::set("USERPROFILE", temp.path().to_str().unwrap());
    let install_config = InstallConfig {
        api_base: Some("https://enterprise.example".to_string()),
        api_key: None,
    };
    let changed =
        persist_install_config_with_values(&test_binary_path(&install_dir), false, &install_config)
            .unwrap();
    assert!(changed);
    let config = crate::config::load_file_config_public().unwrap();
    assert_eq!(
        config.api_base_url.as_deref(),
        Some("https://enterprise.example")
    );
    assert_eq!(config.git_path.as_deref(), Some(expected_git_path));
    assert_eq!(config.api_key, None);
}

#[test]
#[serial]
fn persist_install_config_preserves_existing_git_path() {
    let temp = tempdir().unwrap();
    let install_dir = temp.path().join("bin");
    fs::create_dir_all(&install_dir).unwrap();
    fs::write(test_binary_path(&install_dir), "").unwrap();
    write_install_git_marker(
        &install_dir,
        if cfg!(windows) {
            r"C:\Program Files\Git\bin\git.exe"
        } else {
            "/opt/custom/bin/git"
        },
    );
    let _home = EnvVarGuard::set("HOME", temp.path().to_str().unwrap());
    #[cfg(windows)]
    let _userprofile = EnvVarGuard::set("USERPROFILE", temp.path().to_str().unwrap());
    let existing_git_path = if cfg!(windows) {
        r"D:\PortableGit\bin\git.exe"
    } else {
        "/usr/local/bin/git"
    };
    crate::config::save_file_config(&crate::config::FileConfig {
        git_path: Some(existing_git_path.to_string()),
        ..Default::default()
    })
    .unwrap();
    let install_config = InstallConfig {
        api_base: Some("https://enterprise.example".to_string()),
        api_key: None,
    };
    persist_install_config_with_values(&test_binary_path(&install_dir), false, &install_config)
        .unwrap();
    let config = crate::config::load_file_config_public().unwrap();
    assert_eq!(
        config.api_base_url.as_deref(),
        Some("https://enterprise.example")
    );
    assert_eq!(config.git_path.as_deref(), Some(existing_git_path));
}

#[test]
#[serial]
fn persist_install_config_edge_cases() {
    let temp = tempdir().unwrap();
    let _home = EnvVarGuard::set("HOME", temp.path().to_str().unwrap());
    #[cfg(windows)]
    let _userprofile = EnvVarGuard::set("USERPROFILE", temp.path().to_str().unwrap());
    let install_dir = temp.path().join("bin");
    fs::create_dir_all(&install_dir).unwrap();
    fs::write(test_binary_path(&install_dir), "").unwrap();
    let bin = test_binary_path(&install_dir);
    let load = || crate::config::load_file_config_public().unwrap();
    // (a) empty InstallConfig -> Ok(false), no config file written
    assert!(!persist_install_config_with_values(&bin, false, &Default::default()).unwrap());
    assert!(load().api_base_url.is_none());
    // (b) dry_run=true with values -> Ok(false), no file written
    let v = InstallConfig {
        api_base: Some("https://a.example".to_string()),
        api_key: Some("key1".to_string()),
    };
    assert!(!persist_install_config_with_values(&bin, true, &v).unwrap());
    assert!(load().api_base_url.is_none());
    // (c) both api_base and api_key persisted together in one call
    assert!(persist_install_config_with_values(&bin, false, &v).unwrap());
    let c = load();
    assert_eq!(c.api_base_url.as_deref(), Some("https://a.example"));
    assert_eq!(c.api_key.as_deref(), Some("key1"));
}

#[cfg(windows)]
#[test]
fn parse_git_og_cmd_path_extracts_wrapped_git_path() {
    assert_eq!(
        parse_git_og_cmd_path("@echo off\r\n\"C:\\Program Files\\Git\\bin\\git.exe\" %*\r\n"),
        Some("C:\\Program Files\\Git\\bin\\git.exe".to_string())
    );
}

#[test]
fn minimum_git_version_is_compared_against_the_canonical_parser() {
    assert!(
        crate::operations::git::repository::parse_git_version("git version 2.17.1").unwrap()
            < MIN_GIT_VERSION
    );
    assert!(
        crate::operations::git::repository::parse_git_version("git version 2.22.0").unwrap()
            >= MIN_GIT_VERSION
    );
}
