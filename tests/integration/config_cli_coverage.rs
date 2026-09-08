//! Integration coverage for `git-ai config` keys that previously had no CLI
//! handling: `allow_superuser`, `transcript_streaming_lookback_days`, and
//! `custom_attributes` (including its nested `custom_attributes.<key>` form).
//!
//! These run the real binary against an isolated test HOME, so `config set`
//! writes land in the sandboxed `~/.git-ai/config.json` rather than the user's.

use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::config::{AuthorConfig, FileConfig, NotesBackendConfig};
use serde_json::Value;
use std::collections::HashMap;

/// Parse the JSON emitted by `git-ai config <key>` into a serde value.
fn get_json(repo: &TestRepo, key: &str) -> Value {
    get_json_with_env(repo, key, &[])
}

fn get_json_with_env(repo: &TestRepo, key: &str, envs: &[(&str, &str)]) -> Value {
    let out = repo
        .git_ai_with_env(&["config", key], envs)
        .unwrap_or_else(|e| panic!("config get {key} failed: {e}"));
    serde_json::from_str(out.trim())
        .unwrap_or_else(|e| panic!("config get {key} returned non-JSON {out:?}: {e}"))
}

/// Map a `FileConfig` field name to the CLI key used to read it back, when the
/// two differ. Most fields share a name with their CLI key; the exceptions are
/// enumerated here so the divergence stays explicit and reviewed.
fn cli_key_for_field(field: &str) -> &str {
    match field {
        // `telemetry_oss` is written by the file/CLI but read back as the
        // effective `telemetry_oss_disabled` boolean.
        "telemetry_oss" => "telemetry_oss_disabled",
        other => other,
    }
}

/// Fields that are intentionally mutated only via dot-notation subkeys
/// (e.g. `notes_backend.kind`) and have no bare top-level `set`/`unset` arm.
/// They remain fully readable via `config <field>` and show-all; only the
/// mutation handlers are nested-only. Listed explicitly so the exception is
/// reviewed rather than silently assumed.
fn is_nested_only_for_mutation(field: &str) -> bool {
    matches!(field, "notes_backend")
}

/// Build a `FileConfig` with every field populated so serde emits all of them
/// (Optional fields skip when `None`). This is the source-of-truth field list
/// for the coverage guard below.
fn fully_populated_file_config() -> FileConfig {
    let mut custom_attributes = HashMap::new();
    custom_attributes.insert("k".to_string(), "v".to_string());
    let mut git_ai_hooks = HashMap::new();
    git_ai_hooks.insert(
        "post_notes_updated".to_string(),
        vec!["./hook.sh".to_string()],
    );

    FileConfig {
        git_path: Some("git".to_string()),
        exclude_prompts_in_repositories: Some(vec!["*".to_string()]),
        include_prompts_in_repositories: Some(vec!["*".to_string()]),
        allowed_repositories: Some(vec!["*".to_string()]),
        exclude_repositories: Some(vec!["*".to_string()]),
        telemetry: Some("off".to_string()),
        telemetry_oss: Some("off".to_string()),
        telemetry_enterprise_dsn: Some("https://example.com".to_string()),
        disable_version_checks: Some(true),
        disable_auto_updates: Some(true),
        update_channel: Some("latest".to_string()),
        feature_flags: Some(serde_json::json!({"transcript_sweep": true})),
        api_base_url: Some("https://usegitai.com".to_string()),
        prompt_storage: Some("default".to_string()),
        default_prompt_storage: Some("local".to_string()),
        api_key: Some("secret-key".to_string()),
        quiet: Some(true),
        allow_superuser: Some(true),
        author: Some(AuthorConfig {
            name: Some("Alice".to_string()),
            email: Some("alice@example.com".to_string()),
        }),
        custom_attributes: Some(custom_attributes),
        git_ai_hooks: Some(git_ai_hooks),
        codex_hooks_format: Some("config_toml".to_string()),
        notes_backend: Some(NotesBackendConfig::default()),
        transcript_streaming_lookback_days: Some(7),
        max_checkpoint_file_size_bytes: Some(3 * 1024 * 1024),
        max_checkpoint_total_size_bytes: Some(32 * 1024 * 1024),
        max_checkpoint_total_lines: Some(500_000),
        daemon_memory_limit_mb: Some(1024),
    }
}

fn file_config_field_names() -> Vec<String> {
    let serialized =
        serde_json::to_value(fully_populated_file_config()).expect("serialize FileConfig");
    serialized
        .as_object()
        .expect("FileConfig serializes to an object")
        .keys()
        .cloned()
        .collect()
}

fn run_config(repo: &TestRepo, args: &[&str]) -> std::process::Output {
    // CI=true suppresses the root-superuser stderr warning so the
    // stderr-is-empty assertion holds in containerized runs too.
    repo.git_ai_command_without_pre_sync_for_test(args, &[("CI", "true")])
        .output()
        .unwrap_or_else(|e| panic!("git-ai {args:?} failed to run: {e}"))
}

mod configuration_values;
mod registry_coverage;
