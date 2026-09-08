use super::values::{compile_glob_field, normalize_daemon_memory_limit_mb};
use super::{CodexHooksFormat, ConfigPatch};
use crate::config::Config;
use std::env;

#[cfg(any(test, feature = "test-support"))]
/// Apply test config patch from environment variable (test-only)
/// Reads GIT_AI_TEST_CONFIG_PATCH env var containing JSON and applies patches to config
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn apply_test_config_patch(config: &mut Config) {
    if let Ok(patch_json) = env::var("GIT_AI_TEST_CONFIG_PATCH")
        && let Ok(patch) = serde_json::from_str::<ConfigPatch>(&patch_json)
    {
        if let Some(git_path) = patch.git_path {
            config.git_path = git_path;
        }
        if let Some(patterns) = patch.allowed_repositories {
            config.allowed_repositories =
                compile_glob_field(Some(patterns), "allowed_repositories");
        }
        if let Some(patterns) = patch.exclude_prompts_in_repositories {
            config.exclude_prompts_in_repositories =
                compile_glob_field(Some(patterns), "exclude_prompts_in_repositories");
        }
        if let Some(telemetry_oss_disabled) = patch.telemetry_oss_disabled {
            config.telemetry_oss_disabled = telemetry_oss_disabled;
        }
        if let Some(telemetry) = patch.telemetry {
            match telemetry.trim() {
                "on" => config.telemetry_enabled = true,
                "off" => config.telemetry_enabled = false,
                other => {
                    eprintln!(
                        "Warning: Invalid test telemetry value '{}', ignoring",
                        other
                    );
                }
            }
        }
        if let Some(disable_version_checks) = patch.disable_version_checks {
            config.disable_version_checks = disable_version_checks;
        }
        if let Some(disable_auto_updates) = patch.disable_auto_updates {
            config.disable_auto_updates = disable_auto_updates;
        }
        if let Some(prompt_storage) = patch.prompt_storage {
            // Validate the value
            if matches!(prompt_storage.as_str(), "default" | "notes" | "local") {
                config.prompt_storage = prompt_storage;
            } else {
                eprintln!(
                    "Warning: Invalid test prompt_storage value '{}', ignoring",
                    prompt_storage
                );
            }
        }
        if let Some(custom_attributes) = patch.custom_attributes {
            config.custom_attributes = custom_attributes;
        }
        if let Some(author) = patch.author {
            config.author = author.normalized();
        }
        if let Some(feature_flags_value) = patch.feature_flags
            && let Ok(deserialized) = serde_json::from_value::<
                crate::feature_flags::DeserializableFeatureFlags,
            >(feature_flags_value)
        {
            config.feature_flags = crate::feature_flags::FeatureFlags::merge_with(
                config.feature_flags.clone(),
                deserialized,
            );
        }
        if let Some(codex_hooks_format) = patch.codex_hooks_format {
            if let Some(format) = CodexHooksFormat::from_str(&codex_hooks_format) {
                config.codex_hooks_format = format;
            } else {
                eprintln!(
                    "Warning: Invalid test codex_hooks_format value '{}', ignoring",
                    codex_hooks_format
                );
            }
        }
        if let Some(nb) = patch.notes_backend {
            config.notes_backend.kind = nb.kind;
            if let Some(url) = nb.backend_url {
                config.notes_backend.backend_url = Some(url);
            }
        }
        if let Some(days) = patch.transcript_streaming_lookback_days {
            config.transcript_streaming_lookback_days = if days == 0 { None } else { Some(days) };
        }
        if let Some(max_bytes) = patch.max_checkpoint_file_size_bytes {
            config.max_checkpoint_file_size_bytes = max_bytes;
        }
        if let Some(max_bytes) = patch.max_checkpoint_total_size_bytes {
            config.max_checkpoint_total_size_bytes = max_bytes;
        }
        if let Some(max_lines) = patch.max_checkpoint_total_lines {
            config.max_checkpoint_total_lines = max_lines;
        }
        if let Some(limit_mb) = patch.daemon_memory_limit_mb {
            config.daemon_memory_limit_mb = normalize_daemon_memory_limit_mb(limit_mb);
        }
    }
}
