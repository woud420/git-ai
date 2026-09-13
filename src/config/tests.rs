use std::collections::HashMap;
use std::fs;
use std::path::Path;

use glob::Pattern;

use crate::feature_flags::FeatureFlags;

use super::author::AuthorConfig;
use super::file::CodexHooksFormat;
use super::file::{
    ConfigPatch, FileConfig, UpdateChannel, build_config, parse_file_config_bytes,
    resolve_telemetry_enabled,
};
use super::notes_backend::{NotesBackendConfig, NotesBackendKind};
use super::patterns::remote_matches_patterns;
use super::prompt_storage::PromptStorageMode;
use super::{
    Config, DEFAULT_API_BASE_URL, DEFAULT_MAX_CHECKPOINT_FILE_SIZE_BYTES,
    DEFAULT_MAX_CHECKPOINT_TOTAL_LINES, DEFAULT_MAX_CHECKPOINT_TOTAL_SIZE_BYTES,
    author_config_file_fingerprint,
};

pub(crate) fn create_test_config(
    allowed_repositories: Vec<String>,
    exclude_repositories: Vec<String>,
) -> Config {
    Config {
        git_path: "/usr/bin/git".to_string(),
        exclude_prompts_in_repositories: vec![],
        include_prompts_in_repositories: vec![],
        allowed_repositories: allowed_repositories
            .into_iter()
            .filter_map(|s| Pattern::new(&s).ok())
            .collect(),
        exclude_repositories: exclude_repositories
            .into_iter()
            .filter_map(|s| Pattern::new(&s).ok())
            .collect(),
        telemetry_enabled: false,
        telemetry_oss_disabled: false,
        telemetry_enterprise_dsn: None,
        disable_version_checks: false,
        disable_auto_updates: false,
        update_channel: UpdateChannel::Latest,
        feature_flags: FeatureFlags::default(),
        api_base_url: DEFAULT_API_BASE_URL.to_string(),
        prompt_storage: "default".to_string(),
        default_prompt_storage: None,
        api_key: None,
        quiet: false,
        allow_superuser: false,
        author: AuthorConfig::default(),
        custom_attributes: HashMap::new(),
        git_ai_hooks: HashMap::new(),
        codex_hooks_format: CodexHooksFormat::ConfigToml,
        notes_backend: NotesBackendConfig::default(),
        transcript_streaming_lookback_days: Some(7),
        max_checkpoint_file_size_bytes: DEFAULT_MAX_CHECKPOINT_FILE_SIZE_BYTES,
        max_checkpoint_total_size_bytes: DEFAULT_MAX_CHECKPOINT_TOTAL_SIZE_BYTES,
        max_checkpoint_total_lines: DEFAULT_MAX_CHECKPOINT_TOTAL_LINES,
        daemon_memory_limit_mb: None,
    }
}

#[test]
fn test_author_config_normalizes_empty_fields() {
    let author = AuthorConfig {
        name: Some("  Alice  ".to_string()),
        email: Some("   ".to_string()),
    }
    .normalized();

    assert_eq!(author.name.as_deref(), Some("Alice"));
    assert!(author.email.is_none());
    assert!(!author.is_empty());
}

#[test]
fn test_author_config_empty_when_all_fields_blank() {
    let author = AuthorConfig {
        name: Some("".to_string()),
        email: Some("   ".to_string()),
    }
    .normalized();

    assert!(author.is_empty());
}

#[test]
fn test_author_config_file_fingerprint_detects_same_length_edits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    fs::write(&path, br#"{"author":{"name":"Alice"}}"#).unwrap();
    let first = author_config_file_fingerprint(&path).unwrap();

    fs::write(&path, br#"{"author":{"name":"Carol"}}"#).unwrap();
    let second = author_config_file_fingerprint(&path).unwrap();

    assert_eq!(first.len, second.len);
    assert_ne!(first, second);
}

#[test]
fn test_exclusion_takes_precedence_over_allow() {
    let config = create_test_config(
        vec!["https://github.com/allowed/repo".to_string()],
        vec!["https://github.com/allowed/repo".to_string()],
    );

    let remotes = vec![(
        "origin".to_string(),
        "https://github.com/allowed/repo".to_string(),
    )];
    assert!(!config.is_allowed_repository_with_context(Some(&remotes), None));
}

#[test]
fn test_empty_allowlist_denies_everything() {
    let config = create_test_config(vec![], vec![]);

    // Collection is opt-in: an empty allowlist denies everything.
    assert!(!config.is_allowed_repository_with_context(None, None));
    let remotes = vec![(
        "origin".to_string(),
        "https://github.com/any/repo".to_string(),
    )];
    assert!(!config.is_allowed_repository_with_context(Some(&remotes), None));
    assert!(!config.is_allowed_repository_with_context(None, Some(Path::new("/work/some-repo"))));
}

#[test]
fn test_exclude_without_allow_still_denies() {
    let config = create_test_config(vec![], vec!["https://github.com/excluded/repo".to_string()]);

    // Exclusions do not turn on collection: the allowlist is still empty.
    assert!(!config.is_allowed_repository_with_context(None, None));
    let remotes = vec![(
        "origin".to_string(),
        "https://github.com/unrelated/repo".to_string(),
    )];
    assert!(!config.is_allowed_repository_with_context(Some(&remotes), None));
}

#[test]
fn test_allow_without_exclude() {
    let config = create_test_config(vec!["https://github.com/allowed/repo".to_string()], vec![]);

    // With an allowlist but no repository context, deny.
    assert!(!config.is_allowed_repository_with_context(None, None));

    let allowed_remotes = vec![(
        "origin".to_string(),
        "https://github.com/allowed/repo".to_string(),
    )];
    assert!(config.is_allowed_repository_with_context(Some(&allowed_remotes), None));

    let other_remotes = vec![(
        "origin".to_string(),
        "https://github.com/other/repo".to_string(),
    )];
    assert!(!config.is_allowed_repository_with_context(Some(&other_remotes), None));
}

#[test]
fn test_allowlist_matches_repo_root_paths() {
    let config = create_test_config(vec!["/work/repos".to_string()], vec![]);

    // Exact directory entry allows the repo itself and everything beneath it.
    assert!(config.is_allowed_repository_with_context(None, Some(Path::new("/work/repos"))));
    assert!(
        config.is_allowed_repository_with_context(None, Some(Path::new("/work/repos/project")))
    );
    assert!(config.is_allowed_repository_with_context(
        None,
        Some(Path::new("/work/repos/nested/deep/project"))
    ));
    assert!(!config.is_allowed_repository_with_context(None, Some(Path::new("/work/other"))));
    assert!(
        !config
            .is_allowed_repository_with_context(None, Some(Path::new("/work/repos-other/project")))
    );
}

#[test]
fn test_allowlist_matches_repo_root_glob() {
    let config = create_test_config(vec!["/home/*/projects".to_string()], vec![]);

    assert!(config.is_allowed_repository_with_context(None, Some(Path::new("/home/dev/projects"))));
    assert!(
        config.is_allowed_repository_with_context(None, Some(Path::new("/home/dev/projects/repo")))
    );
    assert!(!config.is_allowed_repository_with_context(None, Some(Path::new("/home/dev/src"))));
}

#[test]
fn test_allowlist_matches_windows_style_path_entries() {
    // Entries written with backslashes are normalized before matching.
    let config = create_test_config(vec!["C:\\Users\\dev\\work".to_string()], vec![]);

    assert!(
        config.is_allowed_repository_with_context(None, Some(Path::new("C:/Users/dev/work/repo")))
    );
    assert!(
        !config.is_allowed_repository_with_context(None, Some(Path::new("C:/Users/dev/other")))
    );
}

#[test]
fn test_exclusion_matches_repo_root_paths() {
    let config = create_test_config(vec!["/work".to_string()], vec!["/work/secret".to_string()]);

    assert!(config.is_allowed_repository_with_context(None, Some(Path::new("/work/repo"))));
    assert!(!config.is_allowed_repository_with_context(None, Some(Path::new("/work/secret/repo"))));
}

#[test]
fn test_remote_allowlist_ignores_unrelated_repo_root() {
    let config = create_test_config(vec!["https://github.com/org/*".to_string()], vec![]);

    // A repo root that matches nothing does not defeat a remote match.
    let remotes = vec![(
        "origin".to_string(),
        "https://github.com/org/repo".to_string(),
    )];
    assert!(
        config.is_allowed_repository_with_context(Some(&remotes), Some(Path::new("/tmp/clone")))
    );
    // And a path-only repo does not match a URL-only allowlist.
    assert!(!config.is_allowed_repository_with_context(None, Some(Path::new("/tmp/clone"))));
}

#[test]
fn test_glob_pattern_wildcard_in_allow() {
    let config = create_test_config(vec!["https://github.com/myorg/*".to_string()], vec![]);

    // Test that the pattern would match (note: we can't easily test with real Repository objects,
    // but the pattern compilation is tested by the fact that create_test_config succeeds)
    assert!(!config.allowed_repositories.is_empty());
    assert!(config.allowed_repositories[0].matches("https://github.com/myorg/repo1"));
    assert!(config.allowed_repositories[0].matches("https://github.com/myorg/repo2"));
    assert!(!config.allowed_repositories[0].matches("https://github.com/other/repo"));
}

#[test]
fn test_glob_pattern_wildcard_in_exclude() {
    let config = create_test_config(vec![], vec!["https://github.com/private/*".to_string()]);

    // Test pattern matching
    assert!(!config.exclude_repositories.is_empty());
    assert!(config.exclude_repositories[0].matches("https://github.com/private/repo1"));
    assert!(config.exclude_repositories[0].matches("https://github.com/private/secret"));
    assert!(!config.exclude_repositories[0].matches("https://github.com/public/repo"));
}

#[test]
fn test_exact_match_still_works() {
    let config = create_test_config(vec!["https://github.com/exact/match".to_string()], vec![]);

    // Test that exact matches still work (glob treats them as literals)
    assert!(!config.allowed_repositories.is_empty());
    assert!(config.allowed_repositories[0].matches("https://github.com/exact/match"));
    assert!(!config.allowed_repositories[0].matches("https://github.com/exact/other"));
}

#[test]
fn test_complex_glob_patterns() {
    let config = create_test_config(vec!["*@github.com:company/*".to_string()], vec![]);

    // Test more complex patterns with wildcards
    assert!(!config.allowed_repositories.is_empty());
    assert!(config.allowed_repositories[0].matches("git@github.com:company/repo"));
    assert!(config.allowed_repositories[0].matches("user@github.com:company/project"));
    assert!(!config.allowed_repositories[0].matches("git@github.com:other/repo"));
}

#[test]
fn test_remote_pattern_matching_normalizes_common_git_url_forms() {
    let scp_patterns = vec![Pattern::new("git@github.com:company/*").unwrap()];
    assert!(remote_matches_patterns(
        &scp_patterns,
        "ssh://git@github.com/company/repo"
    ));
    assert!(remote_matches_patterns(
        &scp_patterns,
        "ssh://git@github.com:22/company/repo"
    ));
    assert!(!remote_matches_patterns(
        &scp_patterns,
        "ssh://git@github.com/other/repo"
    ));
    assert!(remote_matches_patterns(
        &scp_patterns,
        "https://github.com/company/repo"
    ));
    assert!(remote_matches_patterns(
        &scp_patterns,
        "git://github.com/company/repo.git"
    ));

    let ssh_patterns = vec![Pattern::new("ssh://git@github.com/company/*").unwrap()];
    assert!(remote_matches_patterns(
        &ssh_patterns,
        "git@github.com:company/repo"
    ));
    assert!(remote_matches_patterns(
        &ssh_patterns,
        "https://github.com/company/repo.git"
    ));

    let ssh_port_patterns = vec![Pattern::new("ssh://git@github.com:2222/company/*").unwrap()];
    assert!(remote_matches_patterns(
        &ssh_port_patterns,
        "git@github.com:company/repo"
    ));
    assert!(remote_matches_patterns(
        &ssh_port_patterns,
        "ssh://git@github.com:2022/company/repo"
    ));

    let https_patterns = vec![Pattern::new("https://github.com/company/*").unwrap()];
    assert!(remote_matches_patterns(
        &https_patterns,
        "ssh://git@github.com:2022/company/repo"
    ));
    assert!(remote_matches_patterns(
        &https_patterns,
        "git@github.com:company/repo.git"
    ));
}

#[test]
fn test_remote_pattern_matching_allows_hostless_repository_patterns() {
    let patterns = vec![Pattern::new("company/*").unwrap()];

    assert!(remote_matches_patterns(
        &patterns,
        "https://github.com/company/repo"
    ));
    assert!(remote_matches_patterns(
        &patterns,
        "git@gitlab.com:company/repo.git"
    ));
    assert!(!remote_matches_patterns(
        &patterns,
        "https://github.com/other/repo"
    ));
}

#[test]
fn test_remote_pattern_matching_handles_azure_https_and_ssh_shape_difference() {
    let https_patterns = vec![Pattern::new("https://dev.azure.com/acme/widgets/_git/*").unwrap()];
    assert!(remote_matches_patterns(
        &https_patterns,
        "ssh://git@ssh.dev.azure.com:22/v3/acme/widgets/service"
    ));

    let ssh_patterns = vec![Pattern::new("ssh://git@ssh.dev.azure.com/v3/acme/widgets/*").unwrap()];
    assert!(remote_matches_patterns(
        &ssh_patterns,
        "https://dev.azure.com/acme/widgets/_git/service"
    ));
}

// Tests for exclude_prompts_in_repositories (blacklist)

fn create_test_config_with_exclude_prompts(exclude_prompts_patterns: Vec<String>) -> Config {
    Config {
        exclude_prompts_in_repositories: exclude_prompts_patterns
            .into_iter()
            .filter_map(|s| Pattern::new(&s).ok())
            .collect(),
        ..create_test_config(vec![], vec![])
    }
}

#[test]
fn test_should_exclude_prompts_empty_patterns_returns_false() {
    let config = create_test_config_with_exclude_prompts(vec![]);

    // Empty patterns = share everywhere (blacklist model)
    assert!(!config.should_exclude_prompts(&None));
}

#[test]
fn test_should_exclude_prompts_no_repository_returns_false() {
    let config = create_test_config_with_exclude_prompts(vec!["https://github.com/*".to_string()]);

    // Even with patterns, no repository provided = don't exclude (can't verify)
    assert!(!config.should_exclude_prompts(&None));
}

#[test]
fn test_should_exclude_prompts_pattern_matching() {
    let config =
        create_test_config_with_exclude_prompts(vec!["https://github.com/myorg/*".to_string()]);

    // Test that pattern is compiled correctly
    assert!(!config.exclude_prompts_in_repositories.is_empty());
    assert!(config.exclude_prompts_in_repositories[0].matches("https://github.com/myorg/repo1"));
    assert!(config.exclude_prompts_in_repositories[0].matches("https://github.com/myorg/repo2"));
    assert!(!config.exclude_prompts_in_repositories[0].matches("https://github.com/other/repo"));
}

#[test]
fn test_should_exclude_prompts_wildcard_all() {
    let config = create_test_config_with_exclude_prompts(vec!["*".to_string()]);

    // Wildcard * should match any remote URL pattern (exclude all)
    assert!(!config.exclude_prompts_in_repositories.is_empty());
    assert!(config.exclude_prompts_in_repositories[0].matches("https://github.com/any/repo"));
    assert!(config.exclude_prompts_in_repositories[0].matches("git@gitlab.com:any/project"));

    // Wildcard * should also exclude repos without remotes (None case)
    assert!(config.should_exclude_prompts(&None));
}

#[test]
fn test_debug_self_check_remote_bypasses_prompt_exclusion_wildcard() {
    let config = create_test_config_with_exclude_prompts(vec!["*".to_string()]);
    let remotes = vec![(
        "origin".to_string(),
        crate::diagnostic_sentinels::DEBUG_SELF_CHECK_REMOTE_URL.to_string(),
    )];

    assert!(!config.should_exclude_prompts_with_remotes(Some(&remotes)));
}

#[test]
fn test_should_exclude_prompts_local_repo_not_excluded_without_wildcard() {
    // Test 1: Local repo with no patterns configured - never excluded
    let config_no_patterns = create_test_config_with_exclude_prompts(vec![]);
    assert!(!config_no_patterns.should_exclude_prompts(&None));

    // Test 2: Local repo with non-wildcard patterns - not excluded
    // (patterns only match against remotes, local repos have none)
    let config_with_patterns =
        create_test_config_with_exclude_prompts(vec!["https://github.com/*".to_string()]);
    assert!(
        config_with_patterns.exclude_prompts_in_repositories[0]
            .matches("https://github.com/myorg/repo")
    );
    // Non-wildcard patterns should NOT exclude repos without remotes
    assert!(!config_with_patterns.should_exclude_prompts(&None));
}

#[test]
fn test_should_exclude_prompts_respects_patterns_when_remotes_exist() {
    let config =
        create_test_config_with_exclude_prompts(vec!["https://github.com/private/*".to_string()]);

    // Pattern should match private repos (to exclude)
    assert!(config.exclude_prompts_in_repositories[0].matches("https://github.com/private/repo"));
    // Pattern should not match other repos
    assert!(!config.exclude_prompts_in_repositories[0].matches("https://github.com/public/repo"));
}

#[test]
fn test_exclude_prompt_patterns_match_ssh_equivalent_remotes() {
    let config =
        create_test_config_with_exclude_prompts(vec!["git@github.com:private/*".to_string()]);

    assert!(remote_matches_patterns(
        &config.exclude_prompts_in_repositories,
        "ssh://git@github.com/private/repo"
    ));
}

// Tests for effective_prompt_storage() with include_prompts_in_repositories

fn create_test_config_with_include_prompts(
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    prompt_storage: &str,
    default_prompt_storage: Option<&str>,
) -> Config {
    Config {
        exclude_prompts_in_repositories: exclude_patterns
            .into_iter()
            .filter_map(|s| Pattern::new(&s).ok())
            .collect(),
        include_prompts_in_repositories: include_patterns
            .into_iter()
            .filter_map(|s| Pattern::new(&s).ok())
            .collect(),
        prompt_storage: prompt_storage.to_string(),
        default_prompt_storage: default_prompt_storage.map(|s| s.to_string()),
        ..create_test_config(vec![], vec![])
    }
}

mod file_and_backend;
mod prompt_storage_and_remotes;
