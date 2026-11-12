use super::parse::{
    parse_author_config_object, parse_bool, parse_codex_hooks_format,
    parse_custom_attributes_object, parse_git_ai_hooks_object, parse_hook_command_values,
    parse_key_path, parse_notes_backend_kind, parse_value, validate_feature_flags,
    validate_prompt_storage_value,
};
use super::pattern::{
    log_array_changes, repository_list_mut, resolve_repository_value, set_repository_array_field,
};
use super::spec::resolve_key;
use crate::config::{UpdateChannel, mask_api_key};
use serde_json::Value;

/// Store `value` in `slot` and return it as displayed on its `[key]: value` line.
fn store<T: ToString>(slot: &mut Option<T>, value: T) -> String {
    let shown = value.to_string();
    *slot = Some(value);
    shown
}

/// Parse an integer setting. The error names the key and the expected form.
fn parse_integer<T: std::str::FromStr>(
    name: &str,
    value: &str,
    expected: &str,
) -> Result<T, String> {
    value
        .trim()
        .parse()
        .map_err(|_| format!("Invalid {} value '{}'. Expected {}", name, value, expected))
}

pub(super) fn set_config_value(key: &str, value: &str, add_mode: bool) -> Result<(), String> {
    let mut file_config = crate::config::load_file_config_public()?;
    let resolved = resolve_key(key)?;
    let key_path = parse_key_path(key);

    // Handle top-level keys
    if key_path.len() == 1 {
        let name = resolved.spec.name;
        if let Some(list) = repository_list_mut(&mut file_config, name) {
            let added = set_repository_array_field(list, value, add_mode)?;
            crate::config::save_file_config(&file_config)?;
            log_array_changes(&added, add_mode);
            return Ok(());
        }

        // The transcript and metrics budgets must be positive and, unlike the
        // other integer settings, are parsed without trimming.
        let positive_bytes = || {
            let bytes = value.parse::<usize>().ok().filter(|bytes| *bytes > 0);
            bytes.ok_or_else(|| {
                format!(
                    "Invalid {} value '{}'. Expected a positive integer in bytes",
                    name, value
                )
            })
        };

        // Each arm validates and stores the value, and yields what is printed.
        let shown = match name {
            "git_path" => store(&mut file_config.git_path, value.to_string()),
            "telemetry" => {
                if !matches!(value.trim(), "on" | "off") {
                    return Err(format!(
                        "Invalid telemetry value '{}': expected 'on' or 'off'",
                        value
                    ));
                }
                store(&mut file_config.telemetry, value.trim().to_string())
            }
            "telemetry_oss" => store(&mut file_config.telemetry_oss, value.to_string()),
            "telemetry_enterprise_dsn" => {
                store(&mut file_config.telemetry_enterprise_dsn, value.to_string())
            }
            "disable_version_checks" => {
                store(&mut file_config.disable_version_checks, parse_bool(value)?)
            }
            "disable_auto_updates" => {
                store(&mut file_config.disable_auto_updates, parse_bool(value)?)
            }
            "update_channel" => {
                let channel = UpdateChannel::from_str(value).ok_or_else(|| {
                    "Invalid update_channel value. Expected 'latest', 'next', 'enterprise-latest', or 'enterprise-next'".to_string()
                })?;
                store(
                    &mut file_config.update_channel,
                    channel.as_str().to_string(),
                )
            }
            "feature_flags" => {
                if add_mode {
                    return Err("Cannot use --add with feature_flags at top level. Use dot notation: feature_flags.key".to_string());
                }
                // Parse as JSON object
                let json_value: Value = serde_json::from_str(value)
                    .map_err(|e| format!("Invalid JSON for feature_flags: {}", e))?;
                if !json_value.is_object() {
                    return Err("feature_flags must be a JSON object".to_string());
                }
                validate_feature_flags(&json_value)?;
                file_config.feature_flags = Some(json_value);
                value.to_string()
            }
            "api_base_url" => store(&mut file_config.api_base_url, value.to_string()),
            "api_key" => {
                file_config.api_key = Some(value.to_string());
                mask_api_key(value)
            }
            "prompt_storage" => {
                validate_prompt_storage_value(value)?;
                store(&mut file_config.prompt_storage, value.to_string())
            }
            "include_prompts_in_repositories" => {
                let resolved = resolve_repository_value(value)?;
                if add_mode {
                    let mut list = file_config
                        .include_prompts_in_repositories
                        .unwrap_or_default();
                    for pattern in &resolved {
                        if !list.contains(pattern) {
                            list.push(pattern.clone());
                        }
                    }
                    file_config.include_prompts_in_repositories = Some(list);
                } else {
                    file_config.include_prompts_in_repositories = Some(resolved.clone());
                }
                crate::config::save_file_config(&file_config)?;
                for pattern in resolved {
                    println!("[include_prompts_in_repositories]: {}", pattern);
                }
                return Ok(());
            }
            "default_prompt_storage" => {
                validate_prompt_storage_value(value)?;
                store(&mut file_config.default_prompt_storage, value.to_string())
            }
            "quiet" => store(&mut file_config.quiet, parse_bool(value)?),
            "author" => {
                if add_mode {
                    return Err(
                        "Cannot use --add with author. Use author.name or author.email."
                            .to_string(),
                    );
                }
                let author = parse_author_config_object(value)?;
                let shown = serde_json::to_string(&author)
                    .map_err(|e| format!("Failed to serialize author: {}", e))?;
                file_config.author = Some(author).filter(|author| !author.is_empty());
                shown
            }
            "git_ai_hooks" => {
                if add_mode {
                    return Err("Cannot use --add with git_ai_hooks at top level. Use dot notation: git_ai_hooks.post_notes_updated".to_string());
                }
                file_config.git_ai_hooks = Some(parse_git_ai_hooks_object(value)?);
                value.to_string()
            }
            "codex_hooks_format" => {
                let format = parse_codex_hooks_format(value)?;
                store(
                    &mut file_config.codex_hooks_format,
                    format.as_str().to_string(),
                )
            }
            "allow_superuser" => store(&mut file_config.allow_superuser, parse_bool(value)?),
            "transcript_streaming_lookback_days" => store(
                &mut file_config.transcript_streaming_lookback_days,
                parse_integer(name, value, "a non-negative integer (0 = unlimited)")?,
            ),
            "max_checkpoint_file_size_bytes" => store(
                &mut file_config.max_checkpoint_file_size_bytes,
                parse_integer(name, value, "a non-negative integer in bytes")?,
            ),
            "max_checkpoint_total_size_bytes" => store(
                &mut file_config.max_checkpoint_total_size_bytes,
                parse_integer(name, value, "a non-negative integer in bytes")?,
            ),
            "max_checkpoint_total_lines" => store(
                &mut file_config.max_checkpoint_total_lines,
                parse_integer(name, value, "a non-negative integer in lines")?,
            ),
            "max_transcript_line_bytes" => store(
                &mut file_config.max_transcript_line_bytes,
                positive_bytes()?,
            ),
            "max_transcript_batch_bytes" => store(
                &mut file_config.max_transcript_batch_bytes,
                positive_bytes()?,
            ),
            "max_transcript_file_bytes" => store(
                &mut file_config.max_transcript_file_bytes,
                positive_bytes()?,
            ),
            "max_metrics_flush_chunk_bytes" => store(
                &mut file_config.max_metrics_flush_chunk_bytes,
                positive_bytes()?,
            ),
            "daemon_memory_limit_mb" => {
                let limit_mb: u64 = parse_integer(name, value, "a positive integer in MiB")?;
                if limit_mb == 0 || limit_mb > crate::config::MAX_DAEMON_MEMORY_LIMIT_MB {
                    return Err(format!(
                        "Invalid daemon_memory_limit_mb value '{}'. Expected an integer between 1 and {} MiB",
                        value,
                        crate::config::MAX_DAEMON_MEMORY_LIMIT_MB
                    ));
                }
                store(&mut file_config.daemon_memory_limit_mb, limit_mb)
            }
            "custom_attributes" => {
                if add_mode {
                    return Err("Cannot use --add with custom_attributes at top level. Use dot notation: custom_attributes.key".to_string());
                }
                let attrs = parse_custom_attributes_object(value)?;
                // Mirror the `author`/`git_ai_hooks` convention: an empty object
                // is stored as None so the key is omitted from the config file
                // rather than persisted as a redundant `{}`.
                file_config.custom_attributes = if attrs.is_empty() { None } else { Some(attrs) };
                value.to_string()
            }
            _ => return Err(format!("Unknown config key: {}", key)),
        };
        crate::config::save_file_config(&file_config)?;
        println!("[{}]: {}", name, shown);
        return Ok(());
    }

    // Handle nested keys (dot notation) - only for feature_flags
    if resolved.root() == "feature_flags" {
        if key_path.len() < 2 {
            return Err(
                "feature_flags requires a nested key (e.g., feature_flags.some_flag)".to_string(),
            );
        }

        // Get or create feature_flags object
        let mut flags = file_config
            .feature_flags
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

        if !flags.is_object() {
            return Err("feature_flags must be a JSON object".to_string());
        }

        // Navigate to the parent of the key, creating objects on the way. A
        // single-level key (feature_flags.key) has no intermediate segments.
        let nested_key = key_path[1..].join(".");
        let mut current = flags.as_object_mut().unwrap();
        for segment in &key_path[1..key_path.len() - 1] {
            current = current
                .entry(segment.clone())
                .or_insert_with(|| Value::Object(serde_json::Map::new()))
                .as_object_mut()
                .ok_or_else(|| format!("Cannot navigate through non-object at {}", segment))?;
        }
        // On an object, --add is an upsert: the same as a plain set.
        current.insert(key_path.last().unwrap().clone(), parse_value(value));

        validate_feature_flags(&flags)?;
        file_config.feature_flags = Some(flags);
        crate::config::save_file_config(&file_config)?;
        println!("+ [{}]: {}", nested_key, value);
        return Ok(());
    }

    if resolved.root() == "git_ai_hooks" {
        if key_path.len() != 2 {
            return Err(
                "git_ai_hooks requires a hook name (e.g., git_ai_hooks.post_notes_updated)"
                    .to_string(),
            );
        }

        let hook_name = key_path[1].clone();
        let mut hooks = file_config.git_ai_hooks.unwrap_or_default();
        let commands = parse_hook_command_values(value)?;
        let hook_commands = hooks.entry(hook_name.clone()).or_default();
        if !add_mode {
            hook_commands.clear();
        }
        hook_commands.extend(commands.iter().cloned());
        file_config.git_ai_hooks = Some(hooks);
        crate::config::save_file_config(&file_config)?;
        let prefix = if add_mode { "+ " } else { "" };
        for command in commands {
            println!("{}[{}.{}]: {}", prefix, key_path[0], hook_name, command);
        }
        return Ok(());
    }

    if resolved.root() == "notes_backend" {
        if key_path.len() != 2 {
            return Err(
                "notes_backend requires a field name (notes_backend.kind or notes_backend.backend_url)"
                    .to_string(),
            );
        }
        let field = key_path[1].as_str();
        let mut backend = file_config.notes_backend.clone().unwrap_or_default();
        match field {
            "kind" => {
                let kind = parse_notes_backend_kind(value)?;
                backend.kind = kind;
                file_config.notes_backend = Some(backend);
                crate::config::save_file_config(&file_config)?;
                println!("[notes_backend.kind]: {}", kind.as_str());
            }
            "backend_url" => {
                backend.backend_url = Some(value.to_string());
                file_config.notes_backend = Some(backend);
                crate::config::save_file_config(&file_config)?;
                println!("[notes_backend.backend_url]: {}", value);
            }
            other => return Err(format!("Unknown notes_backend field: {}", other)),
        }
        return Ok(());
    }

    if resolved.root() == "author" {
        if add_mode {
            return Err("Cannot use --add with author fields".to_string());
        }
        if key_path.len() != 2 {
            return Err("author requires a field name (author.name or author.email)".to_string());
        }

        let mut author = file_config.author.clone().unwrap_or_default().normalized();
        let normalized_value = value.trim().to_string();
        if normalized_value.is_empty() {
            return Err(format!("author.{} cannot be empty", key_path[1]));
        }
        match key_path[1].as_str() {
            "name" => author.name = Some(normalized_value.clone()),
            "email" => author.email = Some(normalized_value.clone()),
            other => return Err(format!("Unknown author field: {}", other)),
        }

        file_config.author = Some(author);
        crate::config::save_file_config(&file_config)?;
        println!("[author.{}]: {}", key_path[1], normalized_value);
        return Ok(());
    }

    if resolved.root() == "custom_attributes" {
        if key_path.len() != 2 {
            return Err(
                "custom_attributes requires an attribute name (e.g., custom_attributes.team)"
                    .to_string(),
            );
        }
        let attr_name = key_path[1].trim();
        if attr_name.is_empty() {
            return Err("custom_attributes attribute name cannot be empty".to_string());
        }
        let mut attrs = file_config.custom_attributes.unwrap_or_default();
        attrs.insert(attr_name.to_string(), value.to_string());
        file_config.custom_attributes = Some(attrs);
        crate::config::save_file_config(&file_config)?;
        let prefix = if add_mode { "+ " } else { "" };
        println!("{}[custom_attributes.{}]: {}", prefix, attr_name, value);
        return Ok(());
    }

    Err(
        "Nested keys are only supported for feature_flags, git_ai_hooks, notes_backend, author, and custom_attributes"
            .to_string(),
    )
}
