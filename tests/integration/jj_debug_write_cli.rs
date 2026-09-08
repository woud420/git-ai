use crate::debug_context::snapshot;
use crate::jj_debug_cli::error;
use crate::repos::test_repo::{DaemonTestScope, TestRepo};

fn capture(path: &str) -> Vec<String> {
    [
        "capture",
        "--journal",
        path,
        "--json",
        "--expect-source",
        &"11".repeat(32),
        "--expect-initialization-receipt",
        &"22".repeat(32),
        "--expect-baseline",
        &"33".repeat(32),
        "--expect-generation",
        "0",
        "--expect-head",
        &"44".repeat(64),
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[test]
fn jj_debug_write_cli_help_describes_explicit_writes_and_side_effects() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let before = snapshot(repo.path());
    for action in [None, Some("initialize"), Some("capture")] {
        for flag in ["--help", "-h"] {
            let mut args = vec!["debug", "jj"];
            args.extend(action);
            args.push(flag);
            let output = repo
                .git_ai_command_without_pre_sync_for_test(&args, &[])
                .output()
                .unwrap();
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout)
                .unwrap()
                .to_ascii_lowercase();
            for word in [
                "initialize",
                "capture",
                "--journal",
                "--expect-generation",
                "--expect-head",
                "attribution",
                "disabled",
                "journal",
                "policy",
            ] {
                assert!(text.contains(word), "help lacks {word}");
            }
            assert!(text.contains("creat") && text.contains("migrat"));
        }
    }
    assert_eq!(snapshot(repo.path()), before);
}

#[test]
fn jj_debug_write_cli_rejects_invalid_expectations_before_platform_or_storage() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let parent = repo.test_home_path().join("never-created-write-journal");
    let path = parent.join("journal.sqlite");
    let valid = capture(path.to_str().unwrap());
    let mut cases: Vec<Vec<String>> = vec![
        vec!["initialize".into()],
        vec!["initialize".into(), "--json".into()],
        vec![
            "initialize".into(),
            "--journal".into(),
            path.to_str().unwrap().into(),
        ],
        vec!["initialize".into(), "--help".into(), "--json".into()],
        vec!["capture".into(), "--help".into(), "--json".into()],
    ];
    let init = vec![
        "initialize".to_owned(),
        "--journal".into(),
        path.to_str().unwrap().into(),
        "--json".into(),
    ];
    for extra in [
        vec!["--json"],
        vec!["--journal", "other"],
        vec!["--unknown"],
        vec!["extra"],
        vec!["--expect-head", "bad"],
    ] {
        let mut args = init.clone();
        args.extend(extra.into_iter().map(str::to_owned));
        cases.push(args);
    }
    for index in [1usize, 4, 6, 8, 10, 12] {
        let mut args = valid.clone();
        drop(args.drain(index..index + 2));
        cases.push(args);
        let mut args = valid.clone();
        args.extend(valid[index..index + 2].iter().cloned());
        cases.push(args);
        let mut args = valid.clone();
        args[index + 1] = "--json".into();
        cases.push(args);
    }
    let mut args = valid.clone();
    args.remove(3);
    cases.push(args);
    for index in [5usize, 7, 9] {
        for bad in [
            "1".repeat(63),
            "1".repeat(65),
            "AA".repeat(32),
            "gg".repeat(32),
        ] {
            let mut args = valid.clone();
            args[index] = bad;
            cases.push(args);
        }
    }
    for bad in [
        "-1",
        "+1",
        "01",
        " 1",
        "1 ",
        "1.0",
        "9223372036854775807",
        "18446744073709551616",
    ] {
        let mut args = valid.clone();
        args[11] = bad.into();
        cases.push(args);
    }
    for bad in [
        "0".repeat(128),
        "1".repeat(127),
        "1".repeat(129),
        "AA".repeat(64),
        "gg".repeat(64),
    ] {
        let mut args = valid.clone();
        args[13] = bad;
        cases.push(args);
    }
    let mut args = valid.clone();
    for value in 1..=32 {
        args.extend(["--expect-head".into(), format!("{value:0128x}")]);
    }
    cases.push(args);
    for extra in ["--json", "--unknown", "extra", "--help"] {
        let mut args = valid.clone();
        args.push(extra.into());
        cases.push(args);
    }
    let mut args = init;
    args[1] = "--journal=elsewhere".into();
    cases.push(args);
    let before = snapshot(repo.path());
    for args in cases {
        let mut all = vec!["debug", "jj"];
        all.extend(args.iter().map(String::as_str));
        error(
            repo.git_ai_command_without_pre_sync_for_test(&all, &[])
                .output()
                .unwrap(),
            "usage",
        );
        assert!(!parent.exists());
        assert_eq!(snapshot(repo.path()), before);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn jj_debug_write_cli_unsupported_platform_precedes_paths_and_configuration() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let home = repo.test_home_path().join("never-created-write-home");
    let path = home.join("journal.sqlite");
    for args in [
        vec![
            "initialize".to_owned(),
            "--journal".into(),
            path.to_str().unwrap().into(),
            "--json".into(),
        ],
        capture(path.to_str().unwrap()),
    ] {
        let mut all = vec!["debug", "jj"];
        all.extend(args.iter().map(String::as_str));
        let output = repo
            .git_ai_command_without_pre_sync_for_test(&all, &[])
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("GIT_AI_TEST_CONFIG_PATCH", "invalid-json")
            .env("GIT_CONFIG_GLOBAL", home.join("missing-config"))
            .output()
            .unwrap();
        error(output, "unsupported_platform");
        assert!(!home.exists());
    }
}
