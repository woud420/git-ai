use super::parse::parse_key_path;
use super::spec::{config_key_specs, resolve_key};
use crate::config::{Config, FileConfig, REDACTED_VALUE};
use serde_json::{Value, json};

/// The value `git-ai config <name>` prints for a top-level key, or `None` if
/// `name` is not one. Settings without a runtime default are read from the
/// file and are `null` (an empty list for repository patterns) while unset.
fn top_level_value(name: &str, file_config: &FileConfig, runtime_config: &Config) -> Option<Value> {
    let patterns = |list: &Option<Vec<String>>| json!(list.as_deref().unwrap_or_default());
    Some(match name {
        "git_path" => json!(runtime_config.git_cmd()),
        "exclude_prompts_in_repositories" => patterns(&file_config.exclude_prompts_in_repositories),
        "allowed_repositories" => patterns(&file_config.allowed_repositories),
        "exclude_repositories" => patterns(&file_config.exclude_repositories),
        "telemetry" => json!(if runtime_config.telemetry_enabled() {
            "on"
        } else {
            "off"
        }),
        "telemetry_oss" => json!(runtime_config.is_telemetry_oss_disabled()),
        "telemetry_enterprise_dsn" => json!(file_config.telemetry_enterprise_dsn),
        "disable_version_checks" => json!(runtime_config.version_checks_disabled()),
        "disable_auto_updates" => json!(runtime_config.auto_updates_disabled()),
        "update_channel" => json!(runtime_config.update_channel().as_str()),
        // Effective flags, with defaults applied.
        "feature_flags" => json!(runtime_config.get_feature_flags()),
        "api_base_url" => json!(runtime_config.api_base_url()),
        "api_key" => json!(file_config.api_key.as_ref().map(|_| REDACTED_VALUE)),
        "prompt_storage" => json!(runtime_config.prompt_storage()),
        "include_prompts_in_repositories" => patterns(&file_config.include_prompts_in_repositories),
        "default_prompt_storage" => json!(file_config.default_prompt_storage),
        "quiet" => json!(runtime_config.is_quiet()),
        "author" => json!(runtime_config.author()),
        "git_ai_hooks" => json!(runtime_config.git_ai_hooks()),
        "codex_hooks_format" => json!(runtime_config.codex_hooks_format().as_str()),
        "allow_superuser" => json!(runtime_config.allow_superuser()),
        // The runtime normalizes 0 to None (unlimited). Surface unlimited as
        // 0 so it round-trips through `config set`.
        "transcript_streaming_lookback_days" => json!(
            runtime_config
                .transcript_streaming_lookback_days()
                .unwrap_or(0)
        ),
        "max_checkpoint_file_size_bytes" => json!(runtime_config.max_checkpoint_file_size_bytes()),
        "max_checkpoint_total_size_bytes" => {
            json!(runtime_config.max_checkpoint_total_size_bytes())
        }
        "max_checkpoint_total_lines" => json!(runtime_config.max_checkpoint_total_lines()),
        "max_transcript_line_bytes" => json!(runtime_config.max_transcript_line_bytes()),
        "max_transcript_batch_bytes" => json!(runtime_config.max_transcript_batch_bytes()),
        "max_transcript_file_bytes" => json!(runtime_config.max_transcript_file_bytes()),
        "max_metrics_flush_chunk_bytes" => json!(runtime_config.max_metrics_flush_chunk_bytes()),
        "daemon_memory_limit_mb" => json!(runtime_config.daemon_memory_limit_mb()),
        "custom_attributes" => json!(runtime_config.custom_attributes()),
        "notes_backend" => json!(runtime_config.notes_backend()),
        _ => return None,
    })
}

fn print_json(value: &Value) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize value: {}", e))?;
    println!("{}", json);
    Ok(())
}

pub(super) fn show_all_config() -> Result<(), String> {
    let file_config = crate::config::load_file_config_public()?;
    let runtime_config = Config::get();

    let mut effective_config = serde_json::Map::new();
    for spec in config_key_specs() {
        // Settings that only exist in the file are left out while unset.
        let unset = match spec.name {
            "telemetry_enterprise_dsn" => file_config.telemetry_enterprise_dsn.is_none(),
            "include_prompts_in_repositories" => {
                file_config.include_prompts_in_repositories.is_none()
            }
            "default_prompt_storage" => file_config.default_prompt_storage.is_none(),
            "api_key" => file_config.api_key.is_none(),
            _ => false,
        };
        if !unset && let Some(value) = top_level_value(spec.name, &file_config, runtime_config) {
            // The legacy key is listed under the name of the value it reports.
            let name = match spec.name {
                "telemetry_oss" => "telemetry_oss_disabled",
                name => name,
            };
            effective_config.insert(name.to_string(), value);
        }
    }

    print_json(&Value::Object(effective_config))
}

pub(super) fn get_config_value(key: &str) -> Result<(), String> {
    let file_config = crate::config::load_file_config_public()?;
    let runtime_config = Config::get();
    let resolved = resolve_key(key)?;

    let key_path = parse_key_path(key);

    // Handle top-level keys
    if key_path.len() == 1 {
        let value = top_level_value(resolved.spec.name, &file_config, runtime_config)
            .ok_or_else(|| format!("Unknown config key: {}", key))?;
        return print_json(&value);
    }

    // Handle nested keys (dot notation)
    if matches!(resolved.root(), "feature_flags" | "git_ai_hooks")
        && let Some(root) = top_level_value(resolved.root(), &file_config, runtime_config)
    {
        let mut current = &root;
        for segment in &key_path[1..] {
            current = current
                .get(segment)
                .ok_or_else(|| format!("Config key not found: {}", key))?;
        }
        return print_json(current);
    }

    if resolved.root() == "notes_backend" {
        if key_path.len() != 2 {
            return Err(
                "notes_backend requires a field name (notes_backend.kind or notes_backend.backend_url)"
                    .to_string(),
            );
        }
        let nb = runtime_config.notes_backend();
        let value = match key_path[1].as_str() {
            "kind" => json!(nb.kind.as_str()),
            "backend_url" => json!(nb.backend_url),
            other => return Err(format!("Unknown notes_backend field: {}", other)),
        };
        return print_json(&value);
    }

    if resolved.root() == "author" {
        if key_path.len() != 2 {
            return Err("author requires a field name (author.name or author.email)".to_string());
        }
        let author = runtime_config.author();
        let value = match key_path[1].as_str() {
            "name" => json!(author.name),
            "email" => json!(author.email),
            other => return Err(format!("Unknown author field: {}", other)),
        };
        return print_json(&value);
    }

    if resolved.root() == "custom_attributes" {
        if key_path.len() != 2 {
            return Err(
                "custom_attributes requires an attribute name (e.g., custom_attributes.team)"
                    .to_string(),
            );
        }
        let attr_key = key_path[1].trim();
        return print_json(&json!(runtime_config.custom_attributes().get(attr_key)));
    }

    Err(
        "Nested keys are only supported for feature_flags, git_ai_hooks, notes_backend, author, and custom_attributes"
            .to_string(),
    )
}
