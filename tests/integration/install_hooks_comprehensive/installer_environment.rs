use super::{Command, DaemonTestScope, TestRepo, fs, get_binary_path, run_invalid_installer_env};

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
