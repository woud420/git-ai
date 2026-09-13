use super::*;

#[test]
fn test_parse_file_config_bytes_accepts_utf8_bom() {
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(br#"{"git_path":"C:\\Program Files\\Git\\cmd\\git.exe"}"#);

    let parsed = parse_file_config_bytes(&data).expect("BOM-prefixed config should parse");
    assert_eq!(
        parsed.git_path.as_deref(),
        Some(r"C:\Program Files\Git\cmd\git.exe")
    );
}

#[test]
fn test_parse_file_config_bytes_without_bom_still_parses() {
    let data = br#"{"git_path":"/usr/bin/git"}"#;

    let parsed = parse_file_config_bytes(data).expect("regular config should parse");
    assert_eq!(parsed.git_path.as_deref(), Some("/usr/bin/git"));
}

#[test]
fn test_telemetry_resolution_defaults_off() {
    assert!(!resolve_telemetry_enabled(None, None));
    assert!(resolve_telemetry_enabled(Some("on"), None));
    assert!(!resolve_telemetry_enabled(Some("off"), None));
    assert!(!resolve_telemetry_enabled(Some("bogus"), None));
    // Legacy key: only an explicit "on" enables; absence or "off" stays off.
    assert!(resolve_telemetry_enabled(None, Some("on")));
    assert!(!resolve_telemetry_enabled(None, Some("off")));
    // The new key wins over the legacy key.
    assert!(!resolve_telemetry_enabled(Some("off"), Some("on")));
    assert!(resolve_telemetry_enabled(Some("on"), Some("off")));
}

#[test]
fn test_file_config_accepts_legacy_allow_repositories_key() {
    let data = br#"{"allow_repositories":["https://github.com/org/*"]}"#;

    let parsed = parse_file_config_bytes(data).expect("legacy key should parse");
    assert_eq!(
        parsed.allowed_repositories,
        Some(vec!["https://github.com/org/*".to_string()])
    );
}

#[test]
fn test_file_config_accepts_allowed_repositories_key() {
    let data = br#"{"allowed_repositories":["/work/repos"]}"#;

    let parsed = parse_file_config_bytes(data).expect("canonical key should parse");
    assert_eq!(
        parsed.allowed_repositories,
        Some(vec!["/work/repos".to_string()])
    );
}

#[test]
fn test_config_patch_accepts_both_allowlist_keys() {
    let patch: ConfigPatch = serde_json::from_str(r#"{"allowed_repositories":["/a"]}"#).unwrap();
    assert_eq!(patch.allowed_repositories, Some(vec!["/a".to_string()]));

    let legacy: ConfigPatch = serde_json::from_str(r#"{"allow_repositories":["/b"]}"#).unwrap();
    assert_eq!(legacy.allowed_repositories, Some(vec!["/b".to_string()]));
}

#[test]
#[cfg(unix)]
fn test_path_is_git_ai_binary_symlink_to_git_ai() {
    // A symlink `git → git-ai` should be detected as git-ai.
    let dir = tempfile::tempdir().unwrap();
    let git_ai = dir.path().join("git-ai");
    fs::write(&git_ai, "fake-binary").unwrap();
    std::os::unix::fs::symlink(&git_ai, dir.path().join("git")).unwrap();
    assert!(crate::config::file::path_is_git_ai_binary(
        &dir.path().join("git")
    ));
}

#[test]
fn test_path_is_git_ai_binary_real_git_with_sibling_symlink() {
    // A real `git` binary should NOT be flagged just because a `git-ai`
    // symlink exists in the same directory (Docker/server environment).
    let dir = tempfile::tempdir().unwrap();
    let real_git = dir.path().join("git");
    fs::write(&real_git, "real-git-binary").unwrap();
    // git-ai is a different file (or symlink to a different file)
    let git_ai_target = dir.path().join("git-ai-actual");
    fs::write(&git_ai_target, "git-ai-binary").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&git_ai_target, dir.path().join("git-ai")).unwrap();
    #[cfg(unix)]
    assert!(!crate::config::file::path_is_git_ai_binary(&real_git));
}

#[test]
fn test_path_is_git_ai_binary_hardlink() {
    // A hard-linked shim (same inode) should be detected as git-ai.
    let dir = tempfile::tempdir().unwrap();
    let git_ai = dir.path().join("git-ai");
    fs::write(&git_ai, "fake-binary").unwrap();
    #[cfg(unix)]
    {
        let git = dir.path().join("git");
        fs::hard_link(&git_ai, &git).unwrap();
        assert!(crate::config::file::path_is_git_ai_binary(&git));
    }
}

// --- NotesBackendConfig tests ---

#[test]
fn test_notes_backend_config_default_is_sqlite() {
    let cfg = NotesBackendConfig::default();
    assert_eq!(cfg.kind, NotesBackendKind::Sqlite);
    assert!(cfg.backend_url.is_none());
}

#[test]
fn test_notes_backend_kind_roundtrip() {
    // Serialize and deserialize the full notes_backend object
    let json = r#"{"kind": "http", "backend_url": "https://x"}"#;
    let parsed: NotesBackendConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.kind, NotesBackendKind::Http);
    assert_eq!(parsed.backend_url.as_deref(), Some("https://x"));

    let serialized = serde_json::to_string(&parsed).unwrap();
    let reparsed: NotesBackendConfig = serde_json::from_str(&serialized).unwrap();
    assert_eq!(reparsed, parsed);
}

#[test]
fn test_notes_backend_nested_file_config_roundtrip() {
    // Full file config containing notes_backend nested object
    let json = r#"{"notes_backend": {"kind": "http", "backend_url": "https://x"}}"#;
    let parsed: FileConfig = serde_json::from_str(json).unwrap();
    let nb = parsed
        .notes_backend
        .clone()
        .expect("notes_backend should be set");
    assert_eq!(nb.kind, NotesBackendKind::Http);
    assert_eq!(nb.backend_url.as_deref(), Some("https://x"));

    // Round-trip: re-serialize and check key is preserved
    let serialized = serde_json::to_string_pretty(&parsed).unwrap();
    assert!(serialized.contains("notes_backend"));
    assert!(serialized.contains("http"));
}

#[test]
fn test_notes_backend_kind_as_str() {
    assert_eq!(NotesBackendKind::GitNotes.as_str(), "git_notes");
    assert_eq!(NotesBackendKind::Http.as_str(), "http");
}

#[test]
fn test_notes_backend_kind_display() {
    assert_eq!(NotesBackendKind::GitNotes.to_string(), "git_notes");
    assert_eq!(NotesBackendKind::Http.to_string(), "http");
}

#[test]
fn test_notes_backend_url_unset_returns_none() {
    // When backend_url is absent, notes_backend_url() is None. Callers must handle the unconfigured case explicitly.
    let config = create_test_config(vec![], vec![]);
    assert_eq!(config.notes_backend_url(), None);
}

#[test]
fn test_notes_backend_enabled_false_for_git_notes() {
    let config = create_test_config(vec![], vec![]);
    assert!(!config.notes_backend_enabled());
}

#[test]
fn test_notes_backend_kind_env_var_parsing() {
    // Test the parsing logic that build_config() uses for GIT_AI_NOTES_BACKEND_KIND.
    // We mirror the match arm directly rather than calling build_config() to avoid
    // the git-path resolution required by that function.
    let parse_kind = |s: &str| -> Option<NotesBackendKind> {
        match s {
            "http" => Some(NotesBackendKind::Http),
            "git_notes" | "git-notes" => Some(NotesBackendKind::GitNotes),
            "sqlite" => Some(NotesBackendKind::Sqlite),
            _ => None,
        }
    };

    assert_eq!(parse_kind("sqlite"), Some(NotesBackendKind::Sqlite));
    assert_eq!(parse_kind("http"), Some(NotesBackendKind::Http));
    assert_eq!(parse_kind("git_notes"), Some(NotesBackendKind::GitNotes));
    assert_eq!(parse_kind("git-notes"), Some(NotesBackendKind::GitNotes));
    assert_eq!(parse_kind("invalid"), None);
    assert_eq!(parse_kind(""), None);
}

#[test]
fn test_notes_backend_env_var_overrides_file_config_via_fresh() {
    // Verify that GIT_AI_NOTES_BACKEND_KIND=http is correctly resolved in
    // `build_config()`. We call Config::fresh() with the env var set.
    // This test depends on a real git binary being findable (same constraint
    // as all other integration-style config tests).
    let old = std::env::var("GIT_AI_NOTES_BACKEND_KIND").ok();
    unsafe {
        std::env::set_var("GIT_AI_NOTES_BACKEND_KIND", "http");
    }
    let cfg = Config::fresh();
    let result = cfg.notes_backend_kind();
    // Restore the env var before any assertion that might panic
    match old {
        Some(v) => unsafe { std::env::set_var("GIT_AI_NOTES_BACKEND_KIND", v) },
        None => unsafe { std::env::remove_var("GIT_AI_NOTES_BACKEND_KIND") },
    }
    assert_eq!(
        result,
        NotesBackendKind::Http,
        "GIT_AI_NOTES_BACKEND_KIND=http should override the default git_notes"
    );
}

#[test]
fn test_transcript_streaming_lookback_days_default() {
    let config = create_test_config(vec![], vec![]);
    assert_eq!(config.transcript_streaming_lookback_days(), Some(7));
}

#[test]
#[serial_test::serial]
fn test_transcript_streaming_lookback_days_env_override() {
    let previous = std::env::var("GIT_AI_TRANSCRIPT_STREAMING_LOOKBACK_DAYS").ok();
    unsafe { std::env::set_var("GIT_AI_TRANSCRIPT_STREAMING_LOOKBACK_DAYS", "14") };
    let config = build_config();
    let result = config.transcript_streaming_lookback_days;
    match previous {
        Some(v) => unsafe { std::env::set_var("GIT_AI_TRANSCRIPT_STREAMING_LOOKBACK_DAYS", v) },
        None => unsafe { std::env::remove_var("GIT_AI_TRANSCRIPT_STREAMING_LOOKBACK_DAYS") },
    }
    assert_eq!(result, Some(14));
}

#[test]
#[serial_test::serial]
fn test_transcript_streaming_lookback_days_zero_means_unlimited() {
    let previous = std::env::var("GIT_AI_TRANSCRIPT_STREAMING_LOOKBACK_DAYS").ok();
    unsafe { std::env::set_var("GIT_AI_TRANSCRIPT_STREAMING_LOOKBACK_DAYS", "0") };
    let config = build_config();
    let result = config.transcript_streaming_lookback_days;
    match previous {
        Some(v) => unsafe { std::env::set_var("GIT_AI_TRANSCRIPT_STREAMING_LOOKBACK_DAYS", v) },
        None => unsafe { std::env::remove_var("GIT_AI_TRANSCRIPT_STREAMING_LOOKBACK_DAYS") },
    }
    assert_eq!(result, None);
}
