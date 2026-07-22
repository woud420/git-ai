use crate::operations::git::repository::find_repository_in_path;
use dirs;

/// Determines the type of pattern value provided
#[derive(Debug, PartialEq)]
pub(super) enum PatternType {
    /// Global wildcard pattern like "*"
    GlobalWildcard,
    /// URL or git protocol (http://, https://, git@, ssh://, etc.)
    UrlOrGitProtocol,
    /// File path that should be resolved to a repository
    FilePath,
}

/// Detect the type of pattern value
pub(super) fn detect_pattern_type(value: &str) -> PatternType {
    let trimmed = value.trim();

    // Check for global wildcard
    if trimmed == "*" {
        return PatternType::GlobalWildcard;
    }

    // Check for URL or git protocol patterns
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("git@")
        || trimmed.starts_with("ssh://")
        || trimmed.starts_with("git://")
        || trimmed.contains("://")
        || (trimmed.contains('@') && trimmed.contains(':') && !trimmed.starts_with('/'))
    {
        return PatternType::UrlOrGitProtocol;
    }

    // Check for glob patterns with wildcards (but not just "*")
    // These are patterns like "https://github.com/org/*" or "*@github.com:*"
    if trimmed.contains('*') || trimmed.contains('?') || trimmed.contains('[') {
        return PatternType::UrlOrGitProtocol;
    }

    // Otherwise, treat as file path
    PatternType::FilePath
}

/// Resolve a file path to repository remote URLs
/// Returns the remote URLs for the repository at the given path
pub(super) fn resolve_path_to_remotes(path: &str) -> Result<Vec<String>, String> {
    // Expand ~ to home directory
    let expanded_path = if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            format!("{}{}", home.to_string_lossy(), &path[1..])
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    // Try to find repository at path
    let repo = find_repository_in_path(&expanded_path).map_err(|_| {
        format!(
            "No git repository found at path '{}'. Provide a valid repository path, URL, or glob pattern.",
            path
        )
    })?;

    // Get remotes with URLs
    let remotes = repo
        .remotes_with_urls()
        .map_err(|e| format!("Failed to get remotes for repository at '{}': {}", path, e))?;

    // A repository without remotes is stored by its canonical root path,
    // which is matched as a path pattern.
    if remotes.is_empty() {
        return Ok(vec![crate::utils::normalize_to_posix(
            &repo.canonical_workdir().to_string_lossy(),
        )]);
    }

    // Return all remote URLs
    Ok(remotes.into_iter().map(|(_, url)| url).collect())
}

/// Resolve a repository value - returns the actual patterns to store.
/// For file paths, resolves to repository remote URLs.
/// For URLs/patterns, returns as-is.
pub(super) fn resolve_repository_value(value: &str) -> Result<Vec<String>, String> {
    match detect_pattern_type(value) {
        PatternType::GlobalWildcard | PatternType::UrlOrGitProtocol => {
            // Return as-is
            Ok(vec![value.to_string()])
        }
        PatternType::FilePath => {
            // Resolve to repository remote URLs
            resolve_path_to_remotes(value)
        }
    }
}

/// Set array field for repository patterns (exclude_repositories, allowed_repositories,
/// exclude_prompts_in_repositories). Handles the special logic of detecting if a value is:
///  - A global wildcard pattern like "*"
///  - A URL or git protocol pattern
///  - A file path that should be resolved to repository remotes
///
/// Returns the values that were added/set for logging purposes.
pub(super) fn set_repository_array_field(
    field: &mut Option<Vec<String>>,
    value: &str,
    add_mode: bool,
) -> Result<Vec<String>, String> {
    use serde_json::Value;

    // Resolve the value(s) to add
    let values_to_add = resolve_repository_value(value)?;

    if add_mode {
        // Add mode: append to existing array
        let mut arr = field.take().unwrap_or_default();
        let added = values_to_add.clone();
        arr.extend(values_to_add);
        *field = Some(arr);
        Ok(added)
    } else {
        // Set mode: try to parse as JSON array, or use resolved values
        if value.starts_with('[') {
            // Parse as JSON array
            let json_value: Value =
                serde_json::from_str(value).map_err(|e| format!("Invalid JSON array: {}", e))?;
            if let Value::Array(arr) = json_value {
                let mut resolved_values = Vec::new();
                for v in arr {
                    if let Value::String(s) = v {
                        let resolved = resolve_repository_value(&s)?;
                        resolved_values.extend(resolved);
                    } else {
                        return Err("Array must contain only strings".to_string());
                    }
                }
                let added = resolved_values.clone();
                *field = Some(resolved_values);
                Ok(added)
            } else {
                Err("Expected a JSON array".to_string())
            }
        } else {
            // Single value - use the resolved values
            let added = values_to_add.clone();
            *field = Some(values_to_add);
            Ok(added)
        }
    }
}

/// Log array changes with + prefix for add mode, or just list items for set mode
#[allow(clippy::if_same_then_else)]
pub(super) fn log_array_changes(items: &[String], add_mode: bool) {
    if add_mode {
        for item in items {
            println!("+ {}", item);
        }
    } else {
        for item in items {
            println!("+ {}", item);
        }
    }
}

/// Log array removals with - prefix
pub(super) fn log_array_removals(items: &[String]) {
    for item in items {
        println!("- {}", item);
    }
}
