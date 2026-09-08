use super::{HashMap, InstallStatus, to_hashmap};

// ==============================================================================
// InstallStatus Tests
// ==============================================================================

#[test]
fn test_install_status_as_str() {
    assert_eq!(InstallStatus::NotFound.as_str(), "not_found");
    assert_eq!(InstallStatus::Installed.as_str(), "installed");
    assert_eq!(
        InstallStatus::AlreadyInstalled.as_str(),
        "already_installed"
    );
    assert_eq!(InstallStatus::Failed.as_str(), "failed");
}

#[test]
fn test_install_status_equality() {
    assert_eq!(InstallStatus::NotFound, InstallStatus::NotFound);
    assert_eq!(InstallStatus::Installed, InstallStatus::Installed);
    assert_eq!(
        InstallStatus::AlreadyInstalled,
        InstallStatus::AlreadyInstalled
    );
    assert_eq!(InstallStatus::Failed, InstallStatus::Failed);

    assert_ne!(InstallStatus::NotFound, InstallStatus::Installed);
    assert_ne!(InstallStatus::Installed, InstallStatus::Failed);
}

#[test]
fn test_install_status_copy_clone() {
    let status = InstallStatus::Installed;
    let copied = status;
    let cloned = status;

    assert_eq!(status, copied);
    assert_eq!(status, cloned);
    assert_eq!(copied, cloned);
}

// ==============================================================================
// to_hashmap Conversion Tests
// ==============================================================================

#[test]
fn test_to_hashmap_empty() {
    let statuses: HashMap<String, InstallStatus> = HashMap::new();
    let result = to_hashmap(statuses);
    assert!(result.is_empty());
}

#[test]
fn test_to_hashmap_single_entry() {
    let mut statuses = HashMap::new();
    statuses.insert("cursor".to_string(), InstallStatus::Installed);

    let result = to_hashmap(statuses);
    assert_eq!(result.len(), 1);
    assert_eq!(result.get("cursor"), Some(&"installed".to_string()));
}

#[test]
fn test_to_hashmap_multiple_entries() {
    let mut statuses = HashMap::new();
    statuses.insert("cursor".to_string(), InstallStatus::Installed);
    statuses.insert("claude-code".to_string(), InstallStatus::AlreadyInstalled);
    statuses.insert("codex".to_string(), InstallStatus::NotFound);
    statuses.insert("windsurf".to_string(), InstallStatus::Failed);

    let result = to_hashmap(statuses);
    assert_eq!(result.len(), 4);
    assert_eq!(result.get("cursor"), Some(&"installed".to_string()));
    assert_eq!(
        result.get("claude-code"),
        Some(&"already_installed".to_string())
    );
    assert_eq!(result.get("codex"), Some(&"not_found".to_string()));
    assert_eq!(result.get("windsurf"), Some(&"failed".to_string()));
}

#[test]
fn test_to_hashmap_all_statuses() {
    let mut statuses = HashMap::new();
    statuses.insert("not_found".to_string(), InstallStatus::NotFound);
    statuses.insert("installed".to_string(), InstallStatus::Installed);
    statuses.insert("already".to_string(), InstallStatus::AlreadyInstalled);
    statuses.insert("failed".to_string(), InstallStatus::Failed);

    let result = to_hashmap(statuses);
    assert_eq!(result.get("not_found"), Some(&"not_found".to_string()));
    assert_eq!(result.get("installed"), Some(&"installed".to_string()));
    assert_eq!(
        result.get("already"),
        Some(&"already_installed".to_string())
    );
    assert_eq!(result.get("failed"), Some(&"failed".to_string()));
}

#[test]
fn test_install_status_debug_formatting() {
    let status = InstallStatus::Installed;
    let debug_str = format!("{:?}", status);
    assert!(debug_str.contains("Installed"));
}

#[test]
fn test_to_hashmap_preserves_all_keys() {
    let mut statuses = HashMap::new();
    let keys = vec![
        "cursor",
        "claude-code",
        "codex",
        "windsurf",
        "continue-cli",
        "github-copilot",
    ];

    for (idx, key) in keys.iter().enumerate() {
        let status = match idx % 4 {
            0 => InstallStatus::Installed,
            1 => InstallStatus::AlreadyInstalled,
            2 => InstallStatus::NotFound,
            _ => InstallStatus::Failed,
        };
        statuses.insert(key.to_string(), status);
    }

    let result = to_hashmap(statuses);
    assert_eq!(result.len(), keys.len());

    for key in keys {
        assert!(
            result.contains_key(key),
            "Expected key '{}' to be present",
            key
        );
    }
}

// ==============================================================================
// Status String Validation
// ==============================================================================

#[test]
fn test_all_status_strings_are_lowercase() {
    assert!(
        InstallStatus::NotFound
            .as_str()
            .chars()
            .all(|c| !c.is_uppercase())
    );
    assert!(
        InstallStatus::Installed
            .as_str()
            .chars()
            .all(|c| !c.is_uppercase())
    );
    assert!(
        InstallStatus::AlreadyInstalled
            .as_str()
            .chars()
            .all(|c| !c.is_uppercase())
    );
    assert!(
        InstallStatus::Failed
            .as_str()
            .chars()
            .all(|c| !c.is_uppercase())
    );
}

#[test]
fn test_status_strings_use_underscores() {
    // Verify consistent naming convention
    assert!(InstallStatus::NotFound.as_str().contains('_'));
    assert!(InstallStatus::AlreadyInstalled.as_str().contains('_'));
    assert!(!InstallStatus::Installed.as_str().contains('_'));
    assert!(!InstallStatus::Failed.as_str().contains('_'));
}

#[test]
fn test_status_strings_are_valid_identifiers() {
    // Status strings should be suitable for use as keys
    let statuses = [
        InstallStatus::NotFound,
        InstallStatus::Installed,
        InstallStatus::AlreadyInstalled,
        InstallStatus::Failed,
    ];

    for status in &statuses {
        let s = status.as_str();
        assert!(!s.is_empty());
        assert!(!s.contains(' '));
        assert!(!s.contains('-'));
        // Should only contain alphanumeric and underscores
        assert!(s.chars().all(|c| c.is_alphanumeric() || c == '_'));
    }
}

#[test]
fn test_to_hashmap_with_realistic_agent_names() {
    let mut statuses = HashMap::new();
    statuses.insert("cursor".to_string(), InstallStatus::Installed);
    statuses.insert("claude-code".to_string(), InstallStatus::AlreadyInstalled);
    statuses.insert("github-copilot".to_string(), InstallStatus::NotFound);
    statuses.insert("codex".to_string(), InstallStatus::Installed);
    statuses.insert("windsurf".to_string(), InstallStatus::Failed);
    statuses.insert("continue-cli".to_string(), InstallStatus::NotFound);

    let result = to_hashmap(statuses);
    assert_eq!(result.len(), 6);

    // Verify specific mappings
    assert_eq!(result.get("cursor").unwrap(), "installed");
    assert_eq!(result.get("claude-code").unwrap(), "already_installed");
    assert_eq!(result.get("github-copilot").unwrap(), "not_found");
    assert_eq!(result.get("codex").unwrap(), "installed");
    assert_eq!(result.get("windsurf").unwrap(), "failed");
    assert_eq!(result.get("continue-cli").unwrap(), "not_found");
}

#[test]
fn test_hashmap_conversion_stability() {
    // Test that conversion is stable (same input produces same output)
    let mut statuses = HashMap::new();
    statuses.insert("test1".to_string(), InstallStatus::Installed);
    statuses.insert("test2".to_string(), InstallStatus::NotFound);

    let result1 = to_hashmap(statuses.clone());
    let result2 = to_hashmap(statuses);

    assert_eq!(result1.len(), result2.len());
    for (key, value) in result1.iter() {
        assert_eq!(result2.get(key), Some(value));
    }
}
