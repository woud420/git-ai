use crate::repos::test_repo::{DaemonTestScope, TestRepo};
#[cfg(unix)]
use std::fs;
use std::process::Output;

#[path = "sandbox_socket_access/ownership.rs"]
#[cfg(unix)]
mod ownership;

fn run(repo: &TestRepo, args: &[&str], enabled: Option<bool>) -> Output {
    run_with_socket(repo, args, enabled, "active-trace.sock")
}

fn run_with_socket(repo: &TestRepo, args: &[&str], enabled: Option<bool>, socket: &str) -> Output {
    let home = repo.test_home_path();
    let mut command = repo.git_ai_command_without_pre_sync_for_test(args, &[]);
    command
        .env("PATH", "")
        .env("CODEX_HOME", home.join(".codex"))
        .env("CLAUDE_CONFIG_DIR", home.join(".claude"))
        .env("GIT_AI_DAEMON_TRACE_SOCKET", home.join(socket));
    if let Some(enabled) = enabled {
        command.env("GIT_AI_WHITELIST_AGENT_SANDBOXES", enabled.to_string());
    } else {
        command.env_remove("GIT_AI_WHITELIST_AGENT_SANDBOXES");
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    output
}

#[test]
#[cfg(unix)]
fn codex_socket_rotation_cleans_owned_permissions_after_hooks_are_removed() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = repo.test_home_path().join(".codex/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "[features.network_proxy]\nenabled = true\n").unwrap();
    let socket = |name: &str| {
        repo.test_home_path()
            .join(name)
            .to_string_lossy()
            .into_owned()
    };
    run(&repo, &["install-hooks"], Some(true));
    run_with_socket(&repo, &["install-hooks"], Some(true), "next-trace.sock");
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let sockets = &config["features"]["network_proxy"]["unix_sockets"];
    assert!(sockets.get(socket("active-trace.sock")).is_none());
    assert_eq!(
        sockets
            .get(socket("next-trace.sock"))
            .and_then(toml::Value::as_str),
        Some("allow")
    );
    config.as_table_mut().unwrap().remove("hooks");
    fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    let hooks_json = repo.test_home_path().join(".codex/hooks.json");
    if hooks_json.exists() {
        fs::remove_file(hooks_json).unwrap();
    }
    run(&repo, &["uninstall-hooks"], Some(false));
    let config: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert!(
        config["features"]["network_proxy"]["unix_sockets"]
            .get(socket("next-trace.sock"))
            .is_none()
    );
}

#[test]
#[cfg(unix)]
fn codex_user_owned_socket_permission_survives_uninstall() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = repo.test_home_path().join(".codex/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let socket = repo
        .test_home_path()
        .join("active-trace.sock")
        .to_string_lossy()
        .into_owned();
    let initial = toml::Value::try_from(serde_json::json!({"features":{"network_proxy":{"enabled":true,"unix_sockets":{socket.clone():"allow"}}}})).unwrap();
    fs::write(&path, toml::to_string(&initial).unwrap()).unwrap();
    run(&repo, &["install-hooks"], Some(true));
    run(&repo, &["uninstall-hooks"], Some(false));
    let config: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        config["features"]["network_proxy"],
        initial["features"]["network_proxy"]
    );
}

#[test]
#[cfg(unix)]
fn codex_opt_in_preserves_disabled_proxy_and_explicit_socket_denial() {
    for enabled in [false, true] {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        let path = repo.test_home_path().join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let socket = repo
            .test_home_path()
            .join("active-trace.sock")
            .to_string_lossy()
            .into_owned();
        let initial = toml::Value::try_from(serde_json::json!({"features":{"network_proxy":{"enabled":enabled,"unix_sockets":{socket:"deny"}}}})).unwrap();
        fs::write(&path, toml::to_string(&initial).unwrap()).unwrap();
        let output = run(&repo, &["install-hooks"], Some(true));
        let config: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            config["features"]["network_proxy"],
            initial["features"]["network_proxy"]
        );
        let message = String::from_utf8_lossy(&output.stderr).to_string()
            + &String::from_utf8_lossy(&output.stdout);
        assert!(
            message.contains(if enabled { "denied" } else { "network proxy" }),
            "{message}"
        );
    }
}

#[test]
fn socket_permission_opt_in_defaults_off_and_round_trips_through_config() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let key = "feature_flags.whitelist_agent_sandboxes";
    let get = || {
        let output = run(&repo, &["config", key], None);
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    assert_eq!(get(), "false");
    run(&repo, &["config", "set", key, "true"], None);
    assert_eq!(get(), "true");
    run(&repo, &["config", "unset", key], None);
    assert_eq!(get(), "false");
}

#[test]
fn socket_permission_flag_rejects_non_boolean_values_without_changing_config() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let key = "feature_flags.whitelist_agent_sandboxes";
    run(&repo, &["config", "set", key, "true"], None);
    for (key, value) in [
        (key, "not-a-bool"),
        (key, "1"),
        ("feature_flags", r#"{"whitelist_agent_sandboxes":"false"}"#),
        ("feature_flags.whitelist_agent_sandboxes.nested", "false"),
    ] {
        let output = repo
            .git_ai_command_without_pre_sync_for_test(&["config", "set", key, value], &[])
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "accepted {key}={value}: {output:?}"
        );
        let output = run(
            &repo,
            &["config", "feature_flags.whitelist_agent_sandboxes"],
            None,
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "true");
    }
}

#[test]
#[cfg(unix)]
fn codex_socket_permission_is_opt_in_previewable_and_owned_on_uninstall() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = repo.test_home_path().join(".codex/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "[features.network_proxy]\nenabled = true\n[features.network_proxy.domains]\n\"example.com\" = \"allow\"\n[features.network_proxy.unix_sockets]\n\"/tmp/user.sock\" = \"allow\"\n").unwrap();
    let socket = repo
        .test_home_path()
        .join("active-trace.sock")
        .to_string_lossy()
        .into_owned();
    let config = || toml::from_str::<toml::Value>(&fs::read_to_string(&path).unwrap()).unwrap();
    run(&repo, &["install-hooks"], Some(false));
    assert!(
        config()["features"]["network_proxy"]["unix_sockets"]
            .get(&socket)
            .is_none()
    );
    let before = fs::read(&path).unwrap();
    run(&repo, &["install-hooks", "--dry-run"], Some(true));
    assert_eq!(fs::read(&path).unwrap(), before);
    run(&repo, &["install-hooks"], Some(true));
    assert_eq!(
        config()["features"]["network_proxy"]["unix_sockets"][&socket].as_str(),
        Some("allow")
    );
    let installed = fs::read(&path).unwrap();
    run(&repo, &["install-hooks"], Some(true));
    assert_eq!(fs::read(&path).unwrap(), installed);
    run(&repo, &["uninstall-hooks"], Some(false));
    let config = config();
    let network = &config["features"]["network_proxy"];
    assert!(network["unix_sockets"].get(&socket).is_none());
    assert_eq!(
        network["unix_sockets"]["/tmp/user.sock"].as_str(),
        Some("allow")
    );
    assert_eq!(network["domains"]["example.com"].as_str(), Some("allow"));
    assert_eq!(network["enabled"].as_bool(), Some(true));
}

#[test]
#[cfg(target_os = "macos")]
fn claude_socket_permission_preserves_user_sandbox_settings() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = repo.test_home_path().join(".claude/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, r#"{"sandbox":{"enabled":true,"network":{"allowUnixSockets":["/tmp/user.sock"],"allowAllUnixSockets":false}}}"#).unwrap();
    let socket = repo
        .test_home_path()
        .join("active-trace.sock")
        .to_string_lossy()
        .into_owned();
    let config =
        || serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).unwrap();
    run(&repo, &["install-hooks"], Some(true));
    assert_eq!(
        config()["sandbox"]["network"]["allowUnixSockets"],
        serde_json::json!(["/tmp/user.sock", socket])
    );
    run(&repo, &["uninstall-hooks"], Some(false));
    assert_eq!(
        config()["sandbox"],
        serde_json::json!({"enabled":true,"network":{"allowUnixSockets":["/tmp/user.sock"],"allowAllUnixSockets":false}})
    );
}
