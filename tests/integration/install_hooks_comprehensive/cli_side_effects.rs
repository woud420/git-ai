use super::{
    DaemonTestScope, Path, TestRepo, fs, isolated_install_command, seed_pi_uninstall_files,
};

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
