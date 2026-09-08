use super::{
    TestRepo, Value, cli_key_for_field, file_config_field_names, get_json,
    is_nested_only_for_mutation, run_config,
};

#[test]
fn test_config_show_all_includes_new_keys() {
    let repo = TestRepo::new();
    let out = repo.git_ai(&["config"]).expect("show all config");
    let value: Value =
        serde_json::from_str(out.trim()).expect("config show-all should emit valid JSON");

    assert!(value.get("allow_superuser").is_some());
    assert!(value.get("transcript_streaming_lookback_days").is_some());
    assert!(value.get("max_checkpoint_total_size_bytes").is_some());
    assert!(value.get("max_checkpoint_total_lines").is_some());
    assert!(value.get("daemon_memory_limit_mb").is_some());
    assert!(value.get("custom_attributes").is_some());
}

#[test]
fn test_config_registry_preserves_sensitive_alias_and_help_behavior() {
    let repo = TestRepo::new();
    let secret = "api-key-registry-test-secret";

    let set_output = repo
        .git_ai(&["config", "set", "api_key", secret])
        .expect("setting api_key should succeed");
    assert!(set_output.contains("api-...cret"));
    assert!(!set_output.contains(secret));

    let get_output = repo
        .git_ai(&["config", "api_key"])
        .expect("reading api_key should succeed");
    assert!(get_output.contains("api-...cret"));
    assert!(!get_output.contains(secret));

    repo.git_ai(&["config", "set", "telemetry_oss", "off"])
        .expect("setting the legacy telemetry key should succeed");
    assert_eq!(get_json(&repo, "telemetry_oss"), Value::Bool(true));
    assert_eq!(get_json(&repo, "telemetry_oss_disabled"), Value::Bool(true));

    let help = repo
        .git_ai(&["config", "--help"])
        .expect("config help should succeed");
    assert!(help.contains("telemetry_oss                Legacy OSS telemetry setting"));
    assert!(help.contains("author.name                  git-ai author display name override"));
    assert!(help.contains("notes_backend.kind           Notes backend kind"));
}

#[test]
fn test_config_registry_rejects_unknown_key() {
    let repo = TestRepo::new();
    let error = repo
        .git_ai(&["config", "not_a_real_config_key"])
        .expect_err("unknown config key should fail");
    assert!(
        error.contains("Unknown config key"),
        "unexpected error: {error}"
    );
}

/// Regression guard: every persisted `FileConfig` field must be reachable through
/// the `git-ai config` CLI read path (`config <key>`).
///
/// The field list is derived from a fully-populated `FileConfig` via serde rather
/// than hardcoded, so adding a new persisted field WILL break this test until the
/// CLI handlers (and this guard's expectations) are updated. That is the point:
/// CLI read coverage cannot silently regress.
///
/// `get_config_value` has a top-level match arm for every field plus a catch-all
/// that returns "Unknown config key", making it the canonical completeness check:
/// it is the one read path that must handle every field as a bare key (show-all
/// hides unset optionals; mutation handlers are nested-only for some fields).
#[test]
fn test_every_file_config_field_has_cli_get_coverage() {
    let fields = file_config_field_names();

    // Sanity: serde actually emitted every field (none silently skipped).
    assert!(
        fields.len() >= 23,
        "expected all FileConfig fields to serialize, got {}: {:?}",
        fields.len(),
        fields
    );

    let repo = TestRepo::new();
    let mut unknown_to_get = Vec::new();

    for field in &fields {
        let get_key = cli_key_for_field(field);
        // We tolerate any success output; we only fail on the explicit
        // "Unknown config key" rejection produced by the get catch-all.
        match repo.git_ai(&["config", get_key]) {
            Ok(_) => {}
            Err(e) if e.contains("Unknown config key") => {
                unknown_to_get.push(format!("{field} (cli key: {get_key}): {e}"));
            }
            // Other errors (e.g. environment-specific) are not coverage gaps.
            Err(_) => {}
        }
    }

    assert!(
        unknown_to_get.is_empty(),
        "FileConfig fields rejected by `git-ai config <key>` as unknown \
         (add them to get_config_value): {unknown_to_get:?}"
    );
}

/// Regression guard: every persisted `FileConfig` field must be mutable through
/// the `git-ai config unset <key>` write path, except fields documented as
/// nested-only (see `is_nested_only_for_mutation`).
///
/// `unset` needs no value and is non-destructive against the isolated test
/// config, so it cleanly exercises the write handler's top-level coverage. A new
/// field added without a `set`/`unset` arm trips the catch-all here.
#[test]
fn test_every_file_config_field_has_cli_unset_coverage() {
    let repo = TestRepo::new();
    let mut unknown_to_unset = Vec::new();

    for field in &file_config_field_names() {
        if is_nested_only_for_mutation(field) {
            continue;
        }
        match repo.git_ai(&["config", "unset", field]) {
            Ok(_) => {}
            Err(e) if e.contains("Unknown config key") => {
                unknown_to_unset.push(format!("{field}: {e}"));
            }
            Err(_) => {}
        }
    }

    assert!(
        unknown_to_unset.is_empty(),
        "FileConfig fields rejected by `git-ai config unset <key>` as unknown \
         (add them to set_config_value and unset_config_value, or document them \
         in is_nested_only_for_mutation): {unknown_to_unset:?}"
    );
}

#[test]
fn test_config_notes_backend_normal_output_uses_stdout() {
    let repo = TestRepo::new();

    for args in [
        ["config", "set", "notes_backend.kind", "http"].as_slice(),
        [
            "config",
            "set",
            "notes_backend.backend_url",
            "https://example.com",
        ]
        .as_slice(),
        ["config", "notes_backend.kind"].as_slice(),
        ["config", "notes_backend.backend_url"].as_slice(),
        ["config", "unset", "notes_backend.kind"].as_slice(),
        ["config", "unset", "notes_backend.backend_url"].as_slice(),
    ] {
        let output = run_config(&repo, args);
        assert!(
            output.status.success(),
            "git-ai {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.stdout.is_empty(),
            "git-ai {args:?} should write normal output to stdout"
        );
        assert!(
            output.stderr.is_empty(),
            "git-ai {args:?} wrote normal output to stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
