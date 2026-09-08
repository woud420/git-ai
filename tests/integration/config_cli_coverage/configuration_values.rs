use super::{DaemonTestScope, TestRepo, Value, get_json, get_json_with_env};

#[test]
fn eng_374_nix_documented_updates_preserve_other_settings_and_replace_lists() {
    let readme = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("README-nix.md"),
    )
    .unwrap();
    let ownership = readme
        .split("## Configuration ownership")
        .nth(1)
        .expect("Nix guide must explain existing-file ownership")
        .split("\n## ")
        .next()
        .unwrap();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let config_path = repo.test_home_path().join(".git-ai/config.json");
    let mut config: Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    let object = config.as_object_mut().unwrap();
    object.remove("allowed_repositories");
    object.insert("allow_repositories".into(), serde_json::json!(["old-repo"]));
    object.insert(
        "custom_attributes".into(),
        serde_json::json!({"owner": "kept"}),
    );
    let notes_backend = object.get("notes_backend").cloned();
    std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(
        get_json(&repo, "allowed_repositories"),
        serde_json::json!(["old-repo"])
    );

    let mut updated_keys = std::collections::HashSet::new();
    for command in ownership
        .lines()
        .filter_map(|line| line.strip_prefix("git-ai config set "))
    {
        let (key, quoted_json) = command.split_once(' ').unwrap();
        assert!(matches!(
            key,
            "allowed_repositories" | "exclude_repositories"
        ));
        let json = quoted_json
            .strip_prefix('\'')
            .unwrap()
            .strip_suffix('\'')
            .unwrap();
        let expected: Value = serde_json::from_str(json).unwrap();
        repo.git_ai(&["config", "set", key, json]).unwrap();
        assert_eq!(get_json(&repo, key), expected);
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(
            saved[key], expected,
            "set must replace, not append to the list"
        );
        assert_eq!(saved["custom_attributes"], config["custom_attributes"]);
        assert_eq!(saved.get("notes_backend"), notes_backend.as_ref());
        updated_keys.insert(key);
    }
    assert!(updated_keys.contains("allowed_repositories"));
    assert!(updated_keys.contains("exclude_repositories"));
    assert_eq!(
        get_json(&repo, "allowed_repositories"),
        serde_json::json!([]),
        "document revoking collection too"
    );
}

#[test]
fn test_config_allow_superuser_set_get_unset() {
    let repo = TestRepo::new();

    // Default is false.
    assert_eq!(get_json(&repo, "allow_superuser"), Value::Bool(false));

    repo.git_ai(&["config", "set", "allow_superuser", "true"])
        .expect("set allow_superuser");
    assert_eq!(get_json(&repo, "allow_superuser"), Value::Bool(true));

    repo.git_ai(&["config", "unset", "allow_superuser"])
        .expect("unset allow_superuser");
    assert_eq!(get_json(&repo, "allow_superuser"), Value::Bool(false));
}

#[test]
fn test_config_transcript_streaming_lookback_days_set_get_unset() {
    let repo = TestRepo::new();

    // Default is 7 days.
    assert_eq!(
        get_json(&repo, "transcript_streaming_lookback_days"),
        Value::Number(7.into())
    );

    repo.git_ai(&["config", "set", "transcript_streaming_lookback_days", "1"])
        .expect("set lookback to 1");
    assert_eq!(
        get_json(&repo, "transcript_streaming_lookback_days"),
        Value::Number(1.into())
    );

    // 0 means unlimited; runtime normalizes to None and we surface it as 0.
    repo.git_ai(&["config", "set", "transcript_streaming_lookback_days", "0"])
        .expect("set lookback to 0");
    assert_eq!(
        get_json(&repo, "transcript_streaming_lookback_days"),
        Value::Number(0.into())
    );

    // Non-numeric input is rejected.
    assert!(
        repo.git_ai(&["config", "set", "transcript_streaming_lookback_days", "abc"])
            .is_err()
    );

    repo.git_ai(&["config", "unset", "transcript_streaming_lookback_days"])
        .expect("unset lookback");
    assert_eq!(
        get_json(&repo, "transcript_streaming_lookback_days"),
        Value::Number(7.into())
    );
}

#[test]
fn test_config_checkpoint_budget_set_get_unset() {
    let repo = TestRepo::new();

    repo.git_ai(&[
        "config",
        "set",
        "max_checkpoint_total_size_bytes",
        "1048576",
    ])
    .expect("set max_checkpoint_total_size_bytes");
    assert_eq!(
        get_json(&repo, "max_checkpoint_total_size_bytes"),
        Value::Number(1_048_576.into())
    );

    repo.git_ai(&["config", "set", "max_checkpoint_total_lines", "4096"])
        .expect("set max_checkpoint_total_lines");
    assert_eq!(
        get_json(&repo, "max_checkpoint_total_lines"),
        Value::Number(4096.into())
    );

    repo.git_ai(&["config", "unset", "max_checkpoint_total_size_bytes"])
        .expect("unset max_checkpoint_total_size_bytes");
    assert_eq!(
        get_json(&repo, "max_checkpoint_total_size_bytes"),
        Value::Number((32 * 1024 * 1024).into())
    );

    repo.git_ai(&["config", "unset", "max_checkpoint_total_lines"])
        .expect("unset max_checkpoint_total_lines");
    assert_eq!(
        get_json(&repo, "max_checkpoint_total_lines"),
        Value::Number(500_000.into())
    );
}

#[test]
fn test_config_daemon_memory_limit_set_get_unset() {
    let repo = TestRepo::new();

    assert_eq!(
        get_json(&repo, "daemon_memory_limit_mb"),
        Value::Null,
        "daemon memory monitoring should be disabled by default"
    );

    repo.git_ai(&["config", "set", "daemon_memory_limit_mb", "1024"])
        .expect("set daemon_memory_limit_mb");
    assert_eq!(
        get_json(&repo, "daemon_memory_limit_mb"),
        Value::Number(1024.into())
    );

    assert!(
        repo.git_ai(&["config", "set", "daemon_memory_limit_mb", "0"])
            .is_err(),
        "zero must be rejected; unset disables the limit"
    );
    assert!(
        repo.git_ai(&["config", "set", "daemon_memory_limit_mb", "-1"])
            .is_err(),
        "negative limits must be rejected"
    );

    repo.git_ai(&["config", "unset", "daemon_memory_limit_mb"])
        .expect("unset daemon_memory_limit_mb");
    assert_eq!(get_json(&repo, "daemon_memory_limit_mb"), Value::Null);
}

#[test]
fn test_config_custom_attributes_object_set_get_unset() {
    let repo = TestRepo::new();

    // Default is an empty object.
    assert_eq!(
        get_json(&repo, "custom_attributes"),
        Value::Object(serde_json::Map::new())
    );

    repo.git_ai(&[
        "config",
        "set",
        "custom_attributes",
        r#"{"team":"platform","env":"prod"}"#,
    ])
    .expect("set custom_attributes object");

    let value = get_json(&repo, "custom_attributes");
    assert_eq!(value["team"], Value::String("platform".to_string()));
    assert_eq!(value["env"], Value::String("prod".to_string()));

    repo.git_ai(&["config", "unset", "custom_attributes"])
        .expect("unset custom_attributes");
    assert_eq!(
        get_json(&repo, "custom_attributes"),
        Value::Object(serde_json::Map::new())
    );
}

#[test]
fn test_config_custom_attributes_nested_set_get_unset() {
    let repo = TestRepo::new();

    // Set a single attribute via dot notation.
    repo.git_ai(&["config", "set", "custom_attributes.team", "platform"])
        .expect("set custom_attributes.team");
    assert_eq!(
        get_json(&repo, "custom_attributes.team"),
        Value::String("platform".to_string())
    );

    // --add upserts another attribute without clobbering the first.
    repo.git_ai(&["config", "--add", "custom_attributes.env", "prod"])
        .expect("add custom_attributes.env");
    let value = get_json(&repo, "custom_attributes");
    assert_eq!(value["team"], Value::String("platform".to_string()));
    assert_eq!(value["env"], Value::String("prod".to_string()));

    // Unknown nested attribute reads back as null.
    assert_eq!(get_json(&repo, "custom_attributes.missing"), Value::Null);

    // Unset one attribute leaves the other intact.
    repo.git_ai(&["config", "unset", "custom_attributes.team"])
        .expect("unset custom_attributes.team");
    let value = get_json(&repo, "custom_attributes");
    assert!(value.get("team").is_none());
    assert_eq!(value["env"], Value::String("prod".to_string()));

    // Unsetting a missing attribute is an error.
    assert!(
        repo.git_ai(&["config", "unset", "custom_attributes.team"])
            .is_err()
    );
}

#[test]
fn test_config_custom_attributes_set_empty_object_is_omitted() {
    let repo = TestRepo::new();

    // Setting an empty object should normalize to "unset" (mirrors `author`),
    // not persist a redundant `{}`.
    repo.git_ai(&["config", "set", "custom_attributes", "{}"])
        .expect("set empty custom_attributes");
    assert_eq!(
        get_json(&repo, "custom_attributes"),
        Value::Object(serde_json::Map::new())
    );

    // The config file should not carry a `custom_attributes` key at all.
    let config_path = repo.test_home_path().join(".git-ai").join("config.json");
    if let Ok(contents) = std::fs::read_to_string(&config_path) {
        let parsed: Value = serde_json::from_str(&contents).unwrap_or(Value::Null);
        assert!(
            parsed.get("custom_attributes").is_none(),
            "empty custom_attributes should be omitted from config file, got: {contents}"
        );
    }
}

#[test]
fn test_config_custom_attributes_nested_unset_trims_name() {
    let repo = TestRepo::new();

    // Set with a leading space in the attribute name; the set path trims it.
    repo.git_ai(&["config", "set", "custom_attributes. team", "platform"])
        .expect("set custom_attributes. team");
    assert_eq!(
        get_json(&repo, "custom_attributes.team"),
        Value::String("platform".to_string())
    );

    // Get with the same (untrimmed) dotted key must return the stored value.
    assert_eq!(
        get_json(&repo, "custom_attributes. team"),
        Value::String("platform".to_string())
    );

    // Unset with the same (untrimmed) dotted key must succeed symmetrically.
    repo.git_ai(&["config", "unset", "custom_attributes. team"])
        .expect("unset custom_attributes. team should match trimmed name");
    assert_eq!(get_json(&repo, "custom_attributes.team"), Value::Null);
}

#[test]
fn test_config_patch_preserves_unpatched_fields() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let base_git_path = get_json(&repo, "git_path");
    let base_api_url = get_json(&repo, "api_base_url");
    let base_hooks = get_json(&repo, "git_ai_hooks");

    let patch = serde_json::json!({
        "prompt_storage": "local",
        "custom_attributes": { "team": "config-test" },
        "max_checkpoint_total_lines": 1234
    })
    .to_string();
    let envs = [("GIT_AI_TEST_CONFIG_PATCH", patch.as_str())];

    assert_eq!(get_json_with_env(&repo, "git_path", &envs), base_git_path);
    assert_eq!(
        get_json_with_env(&repo, "api_base_url", &envs),
        base_api_url
    );
    assert_eq!(get_json_with_env(&repo, "git_ai_hooks", &envs), base_hooks);
    assert_eq!(get_json_with_env(&repo, "prompt_storage", &envs), "local");
    assert_eq!(
        get_json_with_env(&repo, "custom_attributes.team", &envs),
        "config-test"
    );
    assert_eq!(
        get_json_with_env(&repo, "max_checkpoint_total_lines", &envs),
        1234
    );
}
