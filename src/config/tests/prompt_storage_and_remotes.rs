use super::*;

#[test]
fn test_effective_prompt_storage_no_include_list_uses_global() {
    // No include list = legacy behavior, use global prompt_storage
    let config = create_test_config_with_include_prompts(vec![], vec![], "notes", None);
    assert_eq!(
        config.effective_prompt_storage(&None),
        PromptStorageMode::Notes
    );

    let config = create_test_config_with_include_prompts(vec![], vec![], "local", None);
    assert_eq!(
        config.effective_prompt_storage(&None),
        PromptStorageMode::Local
    );

    let config = create_test_config_with_include_prompts(vec![], vec![], "default", None);
    assert_eq!(
        config.effective_prompt_storage(&None),
        PromptStorageMode::Default
    );
}

#[test]
fn test_effective_prompt_storage_exclude_always_wins() {
    // Exclusion with wildcard should always return Local, regardless of include list
    let config = create_test_config_with_include_prompts(
        vec!["https://github.com/work/*".to_string()],
        vec!["*".to_string()], // Exclude everything
        "default",
        Some("notes"),
    );
    assert_eq!(
        config.effective_prompt_storage(&None),
        PromptStorageMode::Local
    );
}

#[test]
fn test_effective_prompt_storage_wildcard_include_matches_no_repo() {
    // Wildcard include should match repos without remotes (None case)
    let config = create_test_config_with_include_prompts(
        vec!["*".to_string()],
        vec![],
        "default",
        Some("notes"),
    );
    // With wildcard include and None repo, should use prompt_storage (not fallback)
    assert_eq!(
        config.effective_prompt_storage(&None),
        PromptStorageMode::Default
    );
}

#[test]
fn test_effective_prompt_storage_non_wildcard_include_no_match_uses_fallback() {
    // Non-wildcard include with None repo = no match, use fallback
    let config = create_test_config_with_include_prompts(
        vec!["https://github.com/work/*".to_string()],
        vec![],
        "default",
        Some("notes"),
    );
    // None repo can't match non-wildcard pattern, should use default_prompt_storage
    assert_eq!(
        config.effective_prompt_storage(&None),
        PromptStorageMode::Notes
    );
}

#[test]
fn test_effective_prompt_storage_no_fallback_defaults_to_local() {
    // Non-wildcard include with None repo and no fallback = Local
    let config = create_test_config_with_include_prompts(
        vec!["https://github.com/work/*".to_string()],
        vec![],
        "default",
        None, // No fallback configured
    );
    // None repo can't match, and no fallback, should default to Local
    assert_eq!(
        config.effective_prompt_storage(&None),
        PromptStorageMode::Local
    );
}

#[test]
fn test_effective_prompt_storage_include_pattern_matching() {
    let config = create_test_config_with_include_prompts(
        vec!["https://github.com/positron-ai/*".to_string()],
        vec![],
        "default",
        Some("notes"),
    );

    // Test that patterns are compiled correctly
    assert!(!config.include_prompts_in_repositories.is_empty());
    assert!(
        config.include_prompts_in_repositories[0].matches("https://github.com/positron-ai/repo1")
    );
    assert!(
        config.include_prompts_in_repositories[0].matches("https://github.com/positron-ai/project")
    );
    assert!(
        !config.include_prompts_in_repositories[0].matches("https://github.com/other-org/repo")
    );
}

#[test]
fn test_include_prompt_patterns_match_ssh_equivalent_remotes() {
    let config = create_test_config_with_include_prompts(
        vec!["ssh://git@github.com/positron-ai/*".to_string()],
        vec![],
        "default",
        Some("notes"),
    );

    assert!(remote_matches_patterns(
        &config.include_prompts_in_repositories,
        "git@github.com:positron-ai/repo"
    ));
}

#[test]
fn test_prompt_storage_mode_from_str() {
    assert_eq!(
        "default".parse::<PromptStorageMode>().ok(),
        Some(PromptStorageMode::Default)
    );
    assert_eq!(
        "DEFAULT".parse::<PromptStorageMode>().ok(),
        Some(PromptStorageMode::Default)
    );
    assert_eq!(
        "notes".parse::<PromptStorageMode>().ok(),
        Some(PromptStorageMode::Notes)
    );
    assert_eq!(
        "NOTES".parse::<PromptStorageMode>().ok(),
        Some(PromptStorageMode::Notes)
    );
    assert_eq!(
        "local".parse::<PromptStorageMode>().ok(),
        Some(PromptStorageMode::Local)
    );
    assert_eq!(
        "LOCAL".parse::<PromptStorageMode>().ok(),
        Some(PromptStorageMode::Local)
    );
    assert_eq!("invalid".parse::<PromptStorageMode>().ok(), None);
    assert_eq!("".parse::<PromptStorageMode>().ok(), None);
}

#[test]
fn test_prompt_storage_mode_as_str() {
    assert_eq!(PromptStorageMode::Default.as_str(), "default");
    assert_eq!(PromptStorageMode::Notes.as_str(), "notes");
    assert_eq!(PromptStorageMode::Local.as_str(), "local");
}

#[test]
fn test_update_channel_default_is_latest() {
    let channel = UpdateChannel::default();
    assert_eq!(channel, UpdateChannel::Latest);
    assert_eq!(channel.as_str(), "latest");
}

#[test]
fn test_update_channel_enterprise_latest_maps_to_enterprise_latest() {
    let channel = UpdateChannel::from_str("enterprise-latest").unwrap();
    assert_eq!(channel, UpdateChannel::EnterpriseLatest);
    assert_eq!(channel.as_str(), "enterprise-latest");
}

#[test]
fn test_update_channel_enterprise_next_maps_to_enterprise_next() {
    let channel = UpdateChannel::from_str("enterprise-next").unwrap();
    assert_eq!(channel, UpdateChannel::EnterpriseNext);
    assert_eq!(channel.as_str(), "enterprise-next");
}

#[test]
fn test_update_channel_enterprise_latest_parses() {
    let channel = UpdateChannel::from_str("enterprise-latest").unwrap();
    assert_eq!(channel, UpdateChannel::EnterpriseLatest);
    assert_eq!(channel.as_str(), "enterprise-latest");
}

#[test]
fn test_update_channel_enterprise_next_parses() {
    let channel = UpdateChannel::from_str("enterprise-next").unwrap();
    assert_eq!(channel, UpdateChannel::EnterpriseNext);
    assert_eq!(channel.as_str(), "enterprise-next");
}

#[test]
fn test_quiet_default_is_false() {
    let config = create_test_config(vec![], vec![]);
    assert!(!config.is_quiet());
}

#[test]
fn test_quiet_can_be_enabled() {
    let mut config = create_test_config(vec![], vec![]);
    config.quiet = true;
    assert!(config.is_quiet());
}

#[test]
fn test_excluded_repo_with_remotes() {
    let config = create_test_config(vec![], vec!["https://github.com/excluded/*".to_string()]);
    let remotes = vec![(
        "origin".to_string(),
        "https://github.com/excluded/repo".to_string(),
    )];
    assert!(!config.is_allowed_repository_with_context(Some(&remotes), None));
}

#[test]
fn test_allowed_repo_not_excluded_with_remotes() {
    let config = create_test_config(
        vec!["https://github.com/allowed/*".to_string()],
        vec!["https://github.com/excluded/*".to_string()],
    );
    let remotes = vec![(
        "origin".to_string(),
        "https://github.com/allowed/repo".to_string(),
    )];
    assert!(config.is_allowed_repository_with_context(Some(&remotes), None));
}

#[test]
fn test_allowlist_with_remotes() {
    let config = create_test_config(vec!["https://github.com/myorg/*".to_string()], vec![]);
    let remotes = vec![(
        "origin".to_string(),
        "https://github.com/myorg/project".to_string(),
    )];
    assert!(config.is_allowed_repository_with_context(Some(&remotes), None));
}

#[test]
fn test_allowlist_matches_ssh_url_remote_with_scp_pattern() {
    let config = create_test_config(vec!["git@github.com:myorg/*".to_string()], vec![]);
    let remotes = vec![(
        "origin".to_string(),
        "ssh://git@github.com/myorg/project".to_string(),
    )];
    assert!(config.is_allowed_repository_with_context(Some(&remotes), None));
}

#[test]
fn test_allowlist_denies_unmatched_remotes() {
    let config = create_test_config(vec!["https://github.com/myorg/*".to_string()], vec![]);
    let remotes = vec![(
        "origin".to_string(),
        "https://github.com/other/project".to_string(),
    )];
    assert!(!config.is_allowed_repository_with_context(Some(&remotes), None));
}

#[test]
fn test_exclusion_takes_precedence_with_remotes() {
    let config = create_test_config(
        vec!["https://github.com/myorg/*".to_string()],
        vec!["https://github.com/myorg/secret".to_string()],
    );
    let remotes = vec![(
        "origin".to_string(),
        "https://github.com/myorg/secret".to_string(),
    )];
    assert!(!config.is_allowed_repository_with_context(Some(&remotes), None));
}

#[test]
fn test_exclusion_matches_scp_remote_with_ssh_url_pattern() {
    let config = create_test_config(
        vec!["git@github.com:excluded/*".to_string()],
        vec!["ssh://git@github.com/excluded/*".to_string()],
    );
    let remotes = vec![(
        "origin".to_string(),
        "git@github.com:excluded/repo".to_string(),
    )];
    // The remote matches the allowlist, but the normalized exclusion wins.
    assert!(!config.is_allowed_repository_with_context(Some(&remotes), None));
}

#[test]
fn test_no_remotes_denied_with_empty_allowlist() {
    let config = create_test_config(vec![], vec!["https://github.com/excluded/*".to_string()]);
    assert!(!config.is_allowed_repository_with_context(None, None));
}

#[test]
fn test_no_remotes_denied_when_allowlist_active() {
    let config = create_test_config(vec!["https://github.com/myorg/*".to_string()], vec![]);
    assert!(!config.is_allowed_repository_with_context(None, None));
}

#[test]
fn test_empty_remotes_treated_as_no_match_for_exclusion() {
    let config = create_test_config(
        vec!["/work".to_string()],
        vec!["https://github.com/excluded/*".to_string()],
    );
    let remotes: Vec<(String, String)> = vec![];
    // No remotes to exclude; the repo root still satisfies the allowlist.
    assert!(
        config.is_allowed_repository_with_context(Some(&remotes), Some(Path::new("/work/repo")))
    );
}

#[test]
fn test_multiple_remotes_one_excluded() {
    let config = create_test_config(
        vec!["https://github.com/allowed/*".to_string()],
        vec!["https://github.com/excluded/*".to_string()],
    );
    let remotes = vec![
        (
            "origin".to_string(),
            "https://github.com/allowed/repo".to_string(),
        ),
        (
            "upstream".to_string(),
            "https://github.com/excluded/repo".to_string(),
        ),
    ];
    assert!(!config.is_allowed_repository_with_context(Some(&remotes), None));
}
