use crate::debug_context::snapshot;
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use serde_json::{Value, json};
use std::process::Output;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() <= 32 * 1024);
    serde_json::from_slice(&output.stdout).expect("success must be exactly one JSON value")
}

pub(super) fn error(output: Output, code: &str) -> Value {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.len() <= 32 * 1024);
    let value: Value = serde_json::from_slice(&output.stdout)
        .expect("error must be exactly one JSON value on stdout");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["backend"], "jj");
    assert_eq!(value["attribution_enabled"], false);
    assert_eq!(value["error"]["code"], code);
    let message = value["error"]["message"].as_str().unwrap();
    assert!(!message.is_empty());
    assert!(message.len() <= 4096);
    assert_eq!(
        value,
        json!({
            "schema_version":1,"backend":"jj","attribution_enabled":false,
            "error":{"code":code,"message":message}
        })
    );
    value
}

#[test]
fn jj_debug_cli_exact_help_is_platform_independent_and_does_not_create_storage() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let before = snapshot(repo.path());
    for action in [None, Some("status"), Some("receipt")] {
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
            for word in ["status", "receipt", "--journal", "--json", "attribution"] {
                assert!(text.contains(word), "help lacks {word}");
            }
            assert!(text.contains("disabled"));
            assert_eq!(snapshot(repo.path()), before);
        }
    }
}

#[test]
fn jj_debug_cli_strict_usage_is_checked_before_platform_or_storage() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let path = repo
        .test_home_path()
        .join("missing-cli-parent/journal.sqlite");
    let path = path.to_str().unwrap();
    let id = "11".repeat(32);
    let invalid_ids = [
        "1".repeat(63),
        "1".repeat(65),
        "AA".repeat(32),
        "gg".repeat(32),
    ];
    let mut cases: Vec<Vec<&str>> = vec![
        vec![],
        vec!["unknown"],
        vec!["status"],
        vec!["receipt"],
        vec!["status", "--json"],
        vec!["status", "--journal", path],
        vec!["status", "--journal"],
        vec!["status", "--journal", "--json"],
        vec!["status", "--journal", path, "--json", "extra"],
        vec!["status", "--journal", path, "--json", "--json"],
        vec!["status", "--journal", path, "--journal", path, "--json"],
        vec!["status", "--journal", path, "--json", "--source", &id],
        vec!["status", "--journal", path, "--json", "--unknown"],
        vec!["status", "--journal=missing.sqlite", "--json"],
        vec!["--help", "--json"],
        vec!["status", "--help", "--json"],
        vec!["receipt", "-h", "--journal", path],
        vec!["receipt", "--journal", path, "--json"],
        vec!["receipt", "--journal", path, "--source", &id, "--json"],
        vec!["receipt", "--journal", path, "--admission", &id, "--json"],
        vec![
            "receipt",
            "--journal",
            path,
            "--source",
            &id,
            "--admission",
            &id,
        ],
        vec![
            "receipt",
            "--journal",
            path,
            "--source",
            &id,
            "--source",
            &id,
            "--admission",
            &id,
            "--json",
        ],
        vec![
            "receipt",
            "--journal",
            path,
            "--source",
            &id,
            "--admission",
            &id,
            "--admission",
            &id,
            "--json",
        ],
    ];
    for invalid in &invalid_ids {
        cases.push(vec![
            "receipt",
            "--journal",
            path,
            "--source",
            invalid,
            "--admission",
            &id,
            "--json",
        ]);
        cases.push(vec![
            "receipt",
            "--journal",
            path,
            "--source",
            &id,
            "--admission",
            invalid,
            "--json",
        ]);
    }
    let before = snapshot(repo.path());
    for args in cases {
        let mut command_args = vec!["debug", "jj"];
        command_args.extend(args);
        error(
            repo.git_ai_command_without_pre_sync_for_test(&command_args, &[])
                .output()
                .unwrap(),
            "usage",
        );
        assert!(!repo.test_home_path().join("missing-cli-parent").exists());
        assert_eq!(snapshot(repo.path()), before);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn jj_debug_cli_unsupported_platform_precedes_paths_and_configuration() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let home = repo.test_home_path().join("never-created-home");
    let journal = repo
        .test_home_path()
        .join("never-created-journal/db.sqlite");
    let zero = "0".repeat(64);
    for args in [
        vec![
            "debug",
            "jj",
            "status",
            "--journal",
            journal.to_str().unwrap(),
            "--json",
        ],
        vec![
            "debug",
            "jj",
            "receipt",
            "--journal",
            journal.to_str().unwrap(),
            "--source",
            &zero,
            "--admission",
            &zero,
            "--json",
        ],
    ] {
        let output = repo
            .git_ai_command_without_pre_sync_for_test(&args, &[])
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("GIT_AI_TEST_CONFIG_PATCH", "invalid-json")
            .env("GIT_CONFIG_GLOBAL", home.join("missing-config"))
            .output()
            .unwrap();
        error(output, "unsupported_platform");
        assert!(!home.exists());
        assert!(!journal.parent().unwrap().exists());
    }
}
