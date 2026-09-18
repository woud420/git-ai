use super::*;
use serde_json::json;

fn codex(repo: &TestRepo) -> std::path::PathBuf {
    let path = repo.test_home_path().join(".codex/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "[features.network_proxy]\nenabled = true\n").unwrap();
    path
}

#[test]
fn failed_ownership_write_rolls_back_the_new_socket_permission() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = codex(&repo);
    run(&repo, &["install-hooks"], Some(false));
    let before = fs::read(&path).unwrap();
    fs::create_dir(path.with_file_name(".git-ai-sandbox-socket.tmp")).unwrap();
    let output = run(&repo, &["install-hooks"], Some(true));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Error installing extras for Codex"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn corrupt_ownership_record_does_not_change_socket_permissions() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = codex(&repo);
    run(&repo, &["install-hooks"], Some(false));
    let before = fs::read(&path).unwrap();
    let state = path.with_file_name(".git-ai-sandbox-socket.json");
    fs::write(&state, "broken").unwrap();
    let output = run(&repo, &["install-hooks"], Some(true));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Error installing extras for Codex"));
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(fs::read_to_string(state).unwrap(), "broken");
}

#[test]
fn uninstall_preserves_a_user_denial_that_replaced_the_owned_allowance() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = codex(&repo);
    run(&repo, &["install-hooks"], Some(true));
    let socket = repo
        .test_home_path()
        .join("active-trace.sock")
        .to_string_lossy()
        .into_owned();
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    config["features"]["network_proxy"]["unix_sockets"][&socket] =
        toml::Value::String("deny".into());
    fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    run(&repo, &["uninstall-hooks"], Some(false));
    let config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        config["features"]["network_proxy"]["unix_sockets"][&socket].as_str(),
        Some("deny")
    );
    assert!(!path.with_file_name(".git-ai-sandbox-socket.json").exists());
}

#[test]
fn retargeted_config_symlink_does_not_transfer_permission_ownership() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = codex(&repo);
    let original = path.with_file_name("original.toml");
    fs::rename(&path, &original).unwrap();
    std::os::unix::fs::symlink(&original, &path).unwrap();
    run(&repo, &["install-hooks"], Some(true));
    let next = path.with_file_name("user-owned.toml");
    fs::copy(&original, &next).unwrap();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&next, &path).unwrap();
    run(&repo, &["uninstall-hooks"], Some(false));
    let config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let socket = repo
        .test_home_path()
        .join("active-trace.sock")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        config["features"]["network_proxy"]["unix_sockets"][&socket].as_str(),
        Some("allow")
    );
    assert!(path.is_symlink());
}

#[test]
fn malformed_agent_socket_settings_are_preserved() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = codex(&repo);
    fs::write(
        &path,
        "[features.network_proxy]\nenabled = true\nunix_sockets = false\n",
    )
    .unwrap();
    let output = run(&repo, &["install-hooks"], Some(true));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unix_sockets must be a table"));
    let config: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        config["features"]["network_proxy"]["unix_sockets"].as_bool(),
        Some(false)
    );
}

#[test]
#[cfg(target_os = "macos")]
fn claude_preserves_user_owned_permissions_and_previews_owned_cleanup() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = repo.test_home_path().join(".claude/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let socket = repo
        .test_home_path()
        .join("active-trace.sock")
        .to_string_lossy()
        .into_owned();
    let initial = json!({"sandbox":{"enabled":true,"network":{"allowUnixSockets":[socket]}}});
    fs::write(&path, initial.to_string()).unwrap();
    run(&repo, &["install-hooks"], Some(true));
    run_with_socket(&repo, &["install-hooks"], Some(true), "next-trace.sock");
    let before = fs::read(&path).unwrap();
    run(&repo, &["uninstall-hooks", "--dry-run"], Some(false));
    assert_eq!(fs::read(&path).unwrap(), before);
    run(&repo, &["uninstall-hooks"], Some(false));
    let config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(config["sandbox"], initial["sandbox"]);
}

#[test]
#[cfg(not(target_os = "macos"))]
fn claude_does_not_grant_all_sockets_when_path_allowances_are_unsupported() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = repo.test_home_path().join(".claude/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let initial = json!({"sandbox":{"enabled":true,"network":{"allowAllUnixSockets":false}}});
    fs::write(&path, initial.to_string()).unwrap();
    let output = run(&repo, &["install-hooks"], Some(true));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("Unable to configure a path-scoped Unix socket permission")
    );
    let config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(config["sandbox"], initial["sandbox"]);
}
