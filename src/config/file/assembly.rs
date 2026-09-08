use super::git_path::resolve_git_path;
#[cfg(any(test, feature = "test-support"))]
use super::patch::apply_test_config_patch;
use super::storage::{load_file_config, resolve_telemetry_enabled};
use super::values::{compile_glob_field, env_or_file, normalize_daemon_memory_limit_mb};
use super::{CodexHooksFormat, FileConfig, UpdateChannel};
use crate::config::{
    Config, DEFAULT_API_BASE_URL, DEFAULT_MAX_CHECKPOINT_FILE_SIZE_BYTES,
    DEFAULT_MAX_CHECKPOINT_TOTAL_LINES, DEFAULT_MAX_CHECKPOINT_TOTAL_SIZE_BYTES,
    NotesBackendConfig, NotesBackendKind,
};
use crate::feature_flags::FeatureFlags;
use std::collections::HashMap;
use std::env;

pub(crate) fn build_config() -> Config {
    let file_cfg = load_file_config();
    let exclude_prompts_in_repositories = compile_glob_field(
        file_cfg
            .as_ref()
            .and_then(|c| c.exclude_prompts_in_repositories.clone()),
        "exclude_prompts_in_repositories",
    );
    let include_prompts_in_repositories = compile_glob_field(
        file_cfg
            .as_ref()
            .and_then(|c| c.include_prompts_in_repositories.clone()),
        "include_prompts_in_repositories",
    );
    let allowed_repositories = compile_glob_field(
        file_cfg
            .as_ref()
            .and_then(|c| c.allowed_repositories.clone()),
        "allowed_repositories",
    );
    let exclude_repositories = compile_glob_field(
        file_cfg
            .as_ref()
            .and_then(|c| c.exclude_repositories.clone()),
        "exclude_repositories",
    );
    let telemetry_oss_disabled = file_cfg
        .as_ref()
        .and_then(|c| c.telemetry_oss.clone())
        .filter(|s| s == "off")
        .is_some();
    let telemetry_enabled = resolve_telemetry_enabled(
        file_cfg.as_ref().and_then(|c| c.telemetry.as_deref()),
        file_cfg.as_ref().and_then(|c| c.telemetry_oss.as_deref()),
    );
    let telemetry_enterprise_dsn = file_cfg
        .as_ref()
        .and_then(|c| c.telemetry_enterprise_dsn.clone())
        .filter(|s| !s.is_empty());

    // Default to disabled (true) unless this is an OSS build
    // OSS builds set OSS_BUILD env var at compile time to "1", which enables auto-updates by default
    let auto_update_flags_default_disabled = option_env!("OSS_BUILD") != Some("1");

    let disable_version_checks = file_cfg
        .as_ref()
        .and_then(|c| c.disable_version_checks)
        .unwrap_or(auto_update_flags_default_disabled);
    let disable_auto_updates = file_cfg
        .as_ref()
        .and_then(|c| c.disable_auto_updates)
        .unwrap_or(auto_update_flags_default_disabled);
    let update_channel = file_cfg
        .as_ref()
        .and_then(|c| c.update_channel.as_deref())
        .and_then(UpdateChannel::from_str)
        .unwrap_or_default();

    let git_path = resolve_git_path(&file_cfg);

    // Build feature flags from file config
    let feature_flags = build_feature_flags(&file_cfg);

    // Get API base URL from config, env var, or default
    let api_base_url = file_cfg
        .as_ref()
        .and_then(|c| c.api_base_url.clone())
        .or_else(|| env::var("GIT_AI_API_BASE_URL").ok())
        .unwrap_or_else(|| DEFAULT_API_BASE_URL.to_string());

    // Get prompt_storage setting (defaults to "local": prompts stay on this
    // machine unless the user explicitly opts into "notes" or "default"/CAS)
    // Valid values: "default", "notes", "local"
    let prompt_storage = file_cfg
        .as_ref()
        .and_then(|c| c.prompt_storage.clone())
        .unwrap_or_else(|| "local".to_string());
    let prompt_storage = match prompt_storage.as_str() {
        "default" | "notes" | "local" => prompt_storage,
        other => {
            eprintln!(
                "Warning: Invalid prompt_storage value '{}', using 'local'",
                other
            );
            "local".to_string()
        }
    };

    // Get default_prompt_storage setting (fallback for repos not in include list)
    // Valid values: "default", "notes", "local", or None (defaults to "local")
    let default_prompt_storage = file_cfg
        .as_ref()
        .and_then(|c| c.default_prompt_storage.clone())
        .and_then(|s| {
            if matches!(s.as_str(), "default" | "notes" | "local") {
                Some(s)
            } else {
                eprintln!(
                    "Warning: Invalid default_prompt_storage value '{}', ignoring",
                    s
                );
                None
            }
        });

    // Get API key from env var or config file (env var takes precedence)
    let api_key = env::var("GIT_AI_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            file_cfg
                .as_ref()
                .and_then(|c| c.api_key.clone())
                .filter(|s| !s.is_empty())
        });

    // Get quiet setting (defaults to false)
    let quiet = file_cfg.as_ref().and_then(|c| c.quiet).unwrap_or(false);

    let allow_superuser = file_cfg
        .as_ref()
        .and_then(|c| c.allow_superuser)
        .unwrap_or(false);

    let author = file_cfg
        .as_ref()
        .and_then(|c| c.author.clone())
        .unwrap_or_default()
        .normalized();

    // Build custom attributes: file config as base, env var overrides
    let custom_attributes = build_custom_attributes(&file_cfg);

    let git_ai_hooks = file_cfg
        .as_ref()
        .and_then(|c| c.git_ai_hooks.clone())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(hook_name, commands)| {
            let hook_name = hook_name.trim().to_string();
            if hook_name.is_empty() {
                return None;
            }

            let commands: Vec<String> = commands
                .into_iter()
                .map(|command| command.trim().to_string())
                .filter(|command| !command.is_empty())
                .collect();
            if commands.is_empty() {
                return None;
            }

            Some((hook_name, commands))
        })
        .collect::<HashMap<String, Vec<String>>>();

    let codex_hooks_format = file_cfg
        .as_ref()
        .and_then(|c| c.codex_hooks_format.as_deref())
        .and_then(|value| {
            let parsed = CodexHooksFormat::from_str(value);
            if parsed.is_none() {
                eprintln!(
                    "Warning: Invalid codex_hooks_format value '{}', using 'config_toml'",
                    value
                );
            }
            parsed
        })
        .unwrap_or_default();

    // Resolve notes_backend config: env vars override file config, which overrides defaults.
    let file_backend = file_cfg.as_ref().and_then(|c| c.notes_backend.clone());
    let kind_from_env = env::var("GIT_AI_NOTES_BACKEND_KIND")
        .ok()
        .and_then(|s| match s.as_str() {
            "http" => Some(NotesBackendKind::Http),
            "git_notes" | "git-notes" => Some(NotesBackendKind::GitNotes),
            "sqlite" => Some(NotesBackendKind::Sqlite),
            _ => None,
        });
    let url_from_env = env::var("GIT_AI_NOTES_BACKEND_URL").ok();

    // Unconfigured default: sqlite in production. Test builds default to
    // git_notes because in-process test code (which cannot use per-test config
    // patches without racing on process env) predates the sqlite backend and
    // asserts against refs/notes/ai; sqlite-backend behavior is covered by
    // tests that pin the kind explicitly.
    #[cfg(any(test, feature = "test-support"))]
    let unconfigured_kind = NotesBackendKind::GitNotes;
    #[cfg(not(any(test, feature = "test-support")))]
    let unconfigured_kind = NotesBackendKind::default();

    let notes_backend = NotesBackendConfig {
        kind: kind_from_env
            .or_else(|| file_backend.as_ref().map(|b| b.kind))
            .unwrap_or(unconfigured_kind),
        backend_url: url_from_env
            .or_else(|| file_backend.as_ref().and_then(|b| b.backend_url.clone())),
    };

    // Transcript streaming lookback: env > file > default (7 days). 0 means unlimited (None).
    let transcript_streaming_lookback_days = Some(env_or_file(
        "GIT_AI_TRANSCRIPT_STREAMING_LOOKBACK_DAYS",
        file_cfg
            .as_ref()
            .and_then(|c| c.transcript_streaming_lookback_days),
        7u32,
    ))
    .filter(|&v| v != 0);

    // Checkpoint content limits: env > file > defaults.
    let max_checkpoint_file_size_bytes = env_or_file(
        "GIT_AI_MAX_CHECKPOINT_FILE_SIZE_BYTES",
        file_cfg
            .as_ref()
            .and_then(|c| c.max_checkpoint_file_size_bytes),
        DEFAULT_MAX_CHECKPOINT_FILE_SIZE_BYTES,
    );

    let max_checkpoint_total_size_bytes = env_or_file(
        "GIT_AI_MAX_CHECKPOINT_TOTAL_SIZE_BYTES",
        file_cfg
            .as_ref()
            .and_then(|c| c.max_checkpoint_total_size_bytes),
        DEFAULT_MAX_CHECKPOINT_TOTAL_SIZE_BYTES,
    );

    let max_checkpoint_total_lines = env_or_file(
        "GIT_AI_MAX_CHECKPOINT_TOTAL_LINES",
        file_cfg.as_ref().and_then(|c| c.max_checkpoint_total_lines),
        DEFAULT_MAX_CHECKPOINT_TOTAL_LINES,
    );

    let daemon_memory_limit_mb = file_cfg
        .as_ref()
        .and_then(|c| c.daemon_memory_limit_mb)
        .and_then(normalize_daemon_memory_limit_mb);

    let config = Config {
        git_path,
        exclude_prompts_in_repositories,
        include_prompts_in_repositories,
        allowed_repositories,
        exclude_repositories,
        telemetry_enabled,
        telemetry_oss_disabled,
        telemetry_enterprise_dsn,
        disable_version_checks,
        disable_auto_updates,
        update_channel,
        feature_flags,
        api_base_url,
        prompt_storage,
        default_prompt_storage,
        api_key,
        quiet,
        allow_superuser,
        author,
        custom_attributes,
        git_ai_hooks,
        codex_hooks_format,
        notes_backend,
        transcript_streaming_lookback_days,
        max_checkpoint_file_size_bytes,
        max_checkpoint_total_size_bytes,
        max_checkpoint_total_lines,
        daemon_memory_limit_mb,
    };

    #[cfg(any(test, feature = "test-support"))]
    let config = {
        let mut config = config;
        apply_test_config_patch(&mut config);
        config
    };

    config
}

/// Compile a `Vec<String>` glob-pattern config field into `Vec<Pattern>`,
/// dropping (and warning about) any pattern that fails to compile.
/// Build custom attributes from file config and `GIT_AI_CUSTOM_ATTRIBUTES` env var.
/// Env var keys override file config keys on conflict.
pub(super) fn build_custom_attributes(file_cfg: &Option<FileConfig>) -> HashMap<String, String> {
    let mut attrs = file_cfg
        .as_ref()
        .and_then(|c| c.custom_attributes.clone())
        .unwrap_or_default();

    if let Ok(env_val) = env::var("GIT_AI_CUSTOM_ATTRIBUTES") {
        if let Ok(env_attrs) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&env_val)
        {
            for (k, v) in env_attrs {
                match v {
                    serde_json::Value::String(s) => {
                        attrs.insert(k, s);
                    }
                    serde_json::Value::Number(n) => {
                        attrs.insert(k, n.to_string());
                    }
                    serde_json::Value::Bool(b) => {
                        attrs.insert(k, b.to_string());
                    }
                    _ => {} // silently drop arrays, objects, null
                }
            }
        } else {
            tracing::debug!(target: super::TRACING_TARGET, "GIT_AI_CUSTOM_ATTRIBUTES is not valid JSON, ignoring");
        }
    }

    attrs
}

pub(super) fn build_feature_flags(file_cfg: &Option<FileConfig>) -> FeatureFlags {
    let mut file_flags_value = file_cfg
        .as_ref()
        .and_then(|c| c.feature_flags.as_ref())
        .cloned();

    // Backward-compatible alias: accept `feature_flags.globalGitHooks` from config files.
    if let Some(serde_json::Value::Object(ref mut flags)) = file_flags_value
        && let Some(value) = flags.get("globalGitHooks").cloned()
        && !flags.contains_key("global_git_hooks")
    {
        flags.insert("global_git_hooks".to_string(), value);
    }

    // Try to deserialize the feature flags from the JSON value
    let file_flags = file_flags_value.and_then(|value| {
        // Use from_value to deserialize, but ignore any errors and fall back to defaults
        serde_json::from_value(value).ok()
    });

    FeatureFlags::from_env_and_file(file_flags)
}
