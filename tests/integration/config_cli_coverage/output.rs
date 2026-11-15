//! Characterization of `git-ai config` output: what `set` and `unset` print
//! for every top-level key, the errors `set` reports, and how show-all
//! relates to single-key reads.

use super::*;
use std::collections::BTreeMap;

const RESTART_HINT: &str = "Run `git-ai bg restart` for changes to take effect.\n";

/// Stdout of a successful config command, without the daemon restart hint.
fn echo(repo: &TestRepo, args: &[&str]) -> String {
    let output = run_config(repo, args);
    assert!(
        output.status.success(),
        "git-ai {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .replace(RESTART_HINT, "")
}

/// Sets `key`, then unsets it twice, checking what each command prints.
fn assert_echo(repo: &TestRepo, key: &str, value: &str, set: &str, unset: &str) {
    assert_eq!(echo(repo, &["config", "set", key, value]), set, "set {key}");
    assert_eq!(echo(repo, &["config", "unset", key]), unset, "unset {key}");
    assert_eq!(
        echo(repo, &["config", "unset", key]),
        "",
        "unset {key} again"
    );
}

/// Stderr of a config command that must fail without printing to stdout.
fn error(repo: &TestRepo, args: &[&str]) -> String {
    let output = run_config(repo, args);
    assert_eq!(output.status.code(), Some(1), "git-ai {args:?}");
    assert!(output.stdout.is_empty(), "git-ai {args:?}");
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn set_and_unset_echo_each_top_level_key() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let dsn = "https://key@dsn.invalid/1";
    let url = "https://example.invalid/api";
    let flags = r#"{"transcript_sweep": false}"#;
    let author = r#"{"name":"Alice","email":"a@b.c"}"#;
    let hooks = r#"{"post_notes_updated":["./a.sh"]}"#;
    let attrs = r#"{"team":"platform"}"#;

    // Keys echoed as `[key]: value` and `- [key]: value`:
    // (key, value, value echoed by `set`, old value echoed by `unset`)
    let scalars = [
        ("git_path", "/opt/git", "/opt/git", "/opt/git"),
        ("telemetry", " off ", "off", "off"),
        ("telemetry_oss", "off", "off", "off"),
        ("telemetry_enterprise_dsn", dsn, dsn, dsn),
        ("disable_version_checks", "yes", "true", "true"),
        ("disable_auto_updates", "off", "false", "false"),
        ("update_channel", "next", "next", "next"),
        // `set` echoes its argument, `unset` the stored JSON.
        (
            "feature_flags",
            flags,
            flags,
            r#"{"transcript_sweep":false}"#,
        ),
        ("api_base_url", url, url, url),
        ("api_key", "abcdefghijklmnop", "****", "****"),
        ("prompt_storage", "local", "local", "local"),
        ("default_prompt_storage", "local", "local", "local"),
        ("quiet", "1", "true", "true"),
        (
            "author",
            r#"{"name":" Alice ","email":"a@b.c"}"#,
            author,
            author,
        ),
        // `unset` prints maps with Debug formatting.
        (
            "git_ai_hooks",
            hooks,
            hooks,
            r#"{"post_notes_updated": ["./a.sh"]}"#,
        ),
        (
            "codex_hooks_format",
            "hooks-json",
            "hooks_json",
            "hooks_json",
        ),
        ("allow_superuser", "TRUE", "true", "true"),
        ("transcript_streaming_lookback_days", " 3 ", "3", "3"),
        ("max_checkpoint_file_size_bytes", " 1024 ", "1024", "1024"),
        ("max_checkpoint_total_size_bytes", "2048", "2048", "2048"),
        ("max_checkpoint_total_lines", "100", "100", "100"),
        ("max_transcript_line_bytes", "4096", "4096", "4096"),
        ("max_transcript_batch_bytes", "4097", "4097", "4097"),
        ("max_transcript_file_bytes", "4098", "4098", "4098"),
        ("max_metrics_flush_chunk_bytes", "4099", "4099", "4099"),
        ("daemon_memory_limit_mb", " 512 ", "512", "512"),
        ("custom_attributes", attrs, attrs, r#"{"team": "platform"}"#),
    ];
    for (key, value, set, unset) in scalars {
        let set = format!("[{key}]: {set}\n");
        let unset = format!("- [{key}]: {unset}\n");
        assert_echo(&repo, key, value, &set, &unset);
    }

    // Repository lists print one line per pattern, and an alias is echoed
    // under its canonical name: (key, value, `set` output, `unset` output)
    let others = [
        (
            "exclude_prompts_in_repositories",
            "x/*",
            "+ x/*\n",
            "- x/*\n",
        ),
        (
            "allowed_repositories",
            r#"["a/*","b/*"]"#,
            "+ a/*\n+ b/*\n",
            "- a/*\n- b/*\n",
        ),
        ("allow_repositories", "c/*", "+ c/*\n", "- c/*\n"),
        ("exclude_repositories", "p/*", "+ p/*\n", "- p/*\n"),
        (
            "telemetry_oss_disabled",
            "off",
            "[telemetry_oss]: off\n",
            "- [telemetry_oss]: off\n",
        ),
        (
            "include_prompts_in_repositories",
            "i/*",
            "[include_prompts_in_repositories]: i/*\n",
            "- [include_prompts_in_repositories]: [\"i/*\"]\n",
        ),
    ];
    for (key, value, set, unset) in others {
        assert_echo(&repo, key, value, set, unset);
    }
}

#[test]
fn set_reports_invalid_values_and_leaves_the_file_alone() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let config_path = repo.test_home_path().join(".git-ai/config.json");
    let saved = std::fs::read(&config_path).unwrap();

    // Integer settings share one message: (key, value, expected form)
    let integers = [
        (
            "transcript_streaming_lookback_days",
            "-1",
            "a non-negative integer (0 = unlimited)",
        ),
        (
            "max_checkpoint_file_size_bytes",
            "x",
            "a non-negative integer in bytes",
        ),
        (
            "max_checkpoint_total_size_bytes",
            "x",
            "a non-negative integer in bytes",
        ),
        (
            "max_checkpoint_total_lines",
            "x",
            "a non-negative integer in lines",
        ),
        (
            "max_transcript_line_bytes",
            "0",
            "a positive integer in bytes",
        ),
        // Unlike the checkpoint limits, these budgets do not trim the value.
        (
            "max_transcript_batch_bytes",
            " 5",
            "a positive integer in bytes",
        ),
        (
            "max_transcript_file_bytes",
            "x",
            "a positive integer in bytes",
        ),
        (
            "max_metrics_flush_chunk_bytes",
            "-1",
            "a positive integer in bytes",
        ),
        ("daemon_memory_limit_mb", "abc", "a positive integer in MiB"),
        (
            "daemon_memory_limit_mb",
            "0",
            "an integer between 1 and 17592186044415 MiB",
        ),
    ];
    for (key, value, expected) in integers {
        assert_eq!(
            error(&repo, &["config", "set", key, value]),
            format!("Error: Invalid {key} value '{value}'. Expected {expected}\n")
        );
    }

    // (arguments after `config`, error)
    let others: [(&[&str], &str); 22] = [
        (
            &["set", "telemetry", "bogus"],
            "Invalid telemetry value 'bogus': expected 'on' or 'off'",
        ),
        (
            &["set", "disable_version_checks", "maybe"],
            "Invalid boolean value: 'maybe'. Expected true/false",
        ),
        (
            &["set", "disable_auto_updates", " true"],
            "Invalid boolean value: ' true'. Expected true/false",
        ),
        (
            &["set", "quiet", "nope"],
            "Invalid boolean value: 'nope'. Expected true/false",
        ),
        (
            &["set", "allow_superuser", "2"],
            "Invalid boolean value: '2'. Expected true/false",
        ),
        (
            &["set", "update_channel", "bogus"],
            "Invalid update_channel value. Expected 'latest', 'next', 'enterprise-latest', or 'enterprise-next'",
        ),
        (
            &["set", "feature_flags", "[1]"],
            "feature_flags must be a JSON object",
        ),
        (
            &[
                "set",
                "feature_flags",
                r#"{"whitelist_agent_sandboxes":"x"}"#,
            ],
            "feature_flags.whitelist_agent_sandboxes must be true or false",
        ),
        (
            &["set", "prompt_storage", "bogus"],
            "Invalid prompt_storage value 'bogus'. Expected 'default', 'notes', or 'local'",
        ),
        (
            &["set", "default_prompt_storage", "bogus"],
            "Invalid prompt_storage value 'bogus'. Expected 'default', 'notes', or 'local'",
        ),
        (&["set", "author", "[]"], "author must be a JSON object"),
        (
            &["set", "git_ai_hooks", r#"{"h":[]}"#],
            "Hook command array cannot be empty",
        ),
        (
            &["set", "codex_hooks_format", "bogus"],
            "Invalid codex_hooks_format 'bogus'. Expected 'config_toml' or 'hooks_json'",
        ),
        (
            &["set", "custom_attributes", "[1]"],
            "custom_attributes must be a JSON object",
        ),
        (
            &["set", "exclude_repositories", "[1]"],
            "Array must contain only strings",
        ),
        (
            &["set", "notes_backend", "http"],
            "Unknown config key: notes_backend",
        ),
        (
            &["unset", "notes_backend"],
            "Unknown config key: notes_backend",
        ),
        (
            &["--add", "feature_flags", "{}"],
            "Cannot use --add with feature_flags at top level. Use dot notation: feature_flags.key",
        ),
        (
            &["--add", "author", "{}"],
            "Cannot use --add with author. Use author.name or author.email.",
        ),
        (
            &["--add", "git_ai_hooks", "{}"],
            "Cannot use --add with git_ai_hooks at top level. Use dot notation: git_ai_hooks.post_notes_updated",
        ),
        (
            &["--add", "custom_attributes", "{}"],
            "Cannot use --add with custom_attributes at top level. Use dot notation: custom_attributes.key",
        ),
        (&["set", "not_a_key", "1"], "Unknown config key: not_a_key"),
    ];
    for (args, message) in others {
        let args = [&["config"], args].concat();
        assert_eq!(error(&repo, &args), format!("Error: {message}\n"));
    }

    assert_eq!(std::fs::read(&config_path).unwrap(), saved);
}

#[test]
fn show_all_lists_each_key_with_its_get_value() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let show_all = || -> BTreeMap<String, Value> {
        serde_json::from_str(&echo(&repo, &["config"])).expect("show-all should emit JSON")
    };

    // Settings that only exist in the file are left out of show-all while
    // unset, although `config <key>` still answers for them.
    let optional = [
        (
            "telemetry_enterprise_dsn",
            "https://key@dsn.invalid/1",
            Value::Null,
        ),
        (
            "include_prompts_in_repositories",
            "https://github.com/i/*",
            serde_json::json!([]),
        ),
        ("default_prompt_storage", "local", Value::Null),
        ("api_key", "abcdefghijklmnop", Value::Null),
    ];
    let all = show_all();
    for (key, value, unset) in optional {
        assert!(!all.contains_key(key), "{key} is listed while unset");
        assert_eq!(get_json(&repo, key), unset, "{key}");
        repo.git_ai(&["config", "set", key, value]).unwrap();
    }

    // The legacy `telemetry_oss` key is listed under the name of the value it
    // reports, `telemetry_oss_disabled`.
    let all = show_all();
    assert_eq!(
        all.keys().collect::<Vec<_>>(),
        [
            "allow_superuser",
            "allowed_repositories",
            "api_base_url",
            "api_key",
            "author",
            "codex_hooks_format",
            "custom_attributes",
            "daemon_memory_limit_mb",
            "default_prompt_storage",
            "disable_auto_updates",
            "disable_version_checks",
            "exclude_prompts_in_repositories",
            "exclude_repositories",
            "feature_flags",
            "git_ai_hooks",
            "git_path",
            "include_prompts_in_repositories",
            "max_checkpoint_file_size_bytes",
            "max_checkpoint_total_lines",
            "max_checkpoint_total_size_bytes",
            "max_metrics_flush_chunk_bytes",
            "max_transcript_batch_bytes",
            "max_transcript_file_bytes",
            "max_transcript_line_bytes",
            "notes_backend",
            "prompt_storage",
            "quiet",
            "telemetry",
            "telemetry_enterprise_dsn",
            "telemetry_oss_disabled",
            "transcript_streaming_lookback_days",
            "update_channel",
        ]
    );
    for (key, value) in &all {
        assert_eq!(&get_json(&repo, key), value, "{key}");
    }
}
