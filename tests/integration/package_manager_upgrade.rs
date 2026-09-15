use crate::repos::test_repo::{DaemonTestScope, TestRepo, get_binary_path, run_command_output};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn packaged_binary(directory: &Path, manager: Option<&str>) -> PathBuf {
    let binary = directory.join(if cfg!(windows) {
        "git-ai.exe"
    } else {
        "git-ai"
    });
    fs::copy(get_binary_path(), &binary).unwrap();
    if let Some(manager) = manager {
        fs::write(directory.join("git-ai-package-manager"), manager).unwrap();
    }
    binary
}

fn run_packaged(repo: &TestRepo, binary: &Path, args: &[&str]) -> Output {
    let template = repo.git_ai_command_without_pre_sync_for_test(args, &[]);
    let mut command = Command::new(binary);
    command.args(template.get_args()).current_dir(repo.path());
    for (key, value) in template.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        } else {
            command.env_remove(key);
        }
    }
    run_command_output(&mut command, "packaged git-ai").unwrap()
}

fn set_release_api(repo: &TestRepo, url: &str) {
    let path = repo.test_home_path().join(".git-ai/config.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["api_base_url"] = url.into();
    fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
}

#[test]
fn package_manager_upgrade_delegates_without_contacting_release_api() {
    let mut server = mockito::Server::new();
    let release_requests = server.mock("GET", mockito::Matcher::Any).expect(0).create();
    let mut repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    repo.patch_git_ai_config(|patch| {
        patch.disable_auto_updates = Some(false);
        patch.disable_version_checks = Some(false);
    });
    set_release_api(&repo, &server.url());

    for (manager, upgrade_command) in [
        ("homebrew\n", "brew upgrade"),
        ("chocolatey\r\n", "choco upgrade git-ai"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let binary = packaged_binary(directory.path(), Some(manager));
        for args in [&["upgrade"][..], &["upgrade", "--force"][..]] {
            let output = run_packaged(&repo, &binary, args);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(!output.status.success(), "self-upgrade must be refused");
            assert!(stderr.contains(upgrade_command), "{stderr}");
            assert!(!stderr.contains("Failed to check for updates"), "{stderr}");
            assert!(binary.is_file());
        }
        let output = run_packaged(&repo, &binary, &["upgrade", "--background"]);
        assert!(
            output.status.success(),
            "background upgrade should be skipped"
        );
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        #[cfg(unix)]
        {
            let prefix = tempfile::tempdir().unwrap();
            let link = prefix.path().join("git-ai");
            std::os::unix::fs::symlink(&binary, &link).unwrap();
            let output = run_packaged(&repo, &link, &["upgrade"]);
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains(upgrade_command));
        }
        release_requests.assert();
    }
}

#[test]
fn package_manager_upgrade_unmarked_binary_retains_release_check() {
    let mut server = mockito::Server::new();
    let release_request = server
        .mock("GET", mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!({
                "channels": { "latest": {
                    "version": format!("v{}", env!("CARGO_PKG_VERSION")),
                    "checksum": "a".repeat(64)
                }}
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    set_release_api(&repo, &server.url());
    fs::write(repo.path().join("git-ai-package-manager"), "homebrew").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let binary = packaged_binary(directory.path(), None);
    let output = run_packaged(&repo, &binary, &["upgrade"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("already on the latest version"));
    release_request.assert();
}

#[test]
fn package_manager_uninstall_preserves_package_binary_and_user_data() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let directory = tempfile::tempdir().unwrap();
    let binary = packaged_binary(directory.path(), Some("homebrew"));
    let data = repo
        .test_home_path()
        .join(".git-ai")
        .join("attribution-data");
    fs::write(&data, "retained").unwrap();

    let output = run_packaged(&repo, &binary, &["uninstall", "--yes"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(binary.is_file());
    assert!(directory.path().join("git-ai-package-manager").is_file());
    assert_eq!(fs::read_to_string(data).unwrap(), "retained");
}
