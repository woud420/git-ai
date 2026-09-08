use crate::config::MAX_DAEMON_MEMORY_LIMIT_MB;
use glob::Pattern;
use std::env;
use std::str::FromStr;

pub(super) fn normalize_daemon_memory_limit_mb(limit_mb: u64) -> Option<u64> {
    if limit_mb == 0 || limit_mb > MAX_DAEMON_MEMORY_LIMIT_MB {
        eprintln!(
            "Warning: daemon_memory_limit_mb must be between 1 and {MAX_DAEMON_MEMORY_LIMIT_MB} MiB; memory monitoring is disabled"
        );
        return None;
    }
    Some(limit_mb)
}

pub(super) fn compile_glob_field(
    patterns_opt: Option<Vec<String>>,
    field_name: &str,
) -> Vec<Pattern> {
    patterns_opt
        .unwrap_or_default()
        .into_iter()
        .filter_map(|pattern_str| {
            Pattern::new(&pattern_str)
                .map_err(|e| {
                    eprintln!(
                        "Warning: Invalid glob pattern in {} '{}': {}",
                        field_name, pattern_str, e
                    );
                })
                .ok()
        })
        .collect()
}

/// Resolve a numeric config value: env var > file config > default.
pub(super) fn env_or_file<T: FromStr>(env_key: &str, file_val: Option<T>, default: T) -> T {
    env::var(env_key)
        .ok()
        .and_then(|v| v.parse::<T>().ok())
        .or(file_val)
        .unwrap_or(default)
}
