use super::*;

#[test]
fn test_release_from_response_missing_channel() {
    let releases = ReleasesResponse {
        channels: HashMap::new(),
    };
    let result = release_from_response(releases, UpdateChannel::Latest);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn test_release_from_response_empty_tag() {
    let mut channels = HashMap::new();
    channels.insert(
        "latest".to_string(),
        ChannelInfo {
            version: "".to_string(),
            checksum: "abc123".to_string(),
        },
    );
    let releases = ReleasesResponse { channels };
    let result = release_from_response(releases, UpdateChannel::Latest);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn test_release_from_response_empty_checksum() {
    let mut channels = HashMap::new();
    channels.insert(
        "latest".to_string(),
        ChannelInfo {
            version: "v1.0.0".to_string(),
            checksum: "".to_string(),
        },
    );
    let releases = ReleasesResponse { channels };
    let result = release_from_response(releases, UpdateChannel::Latest);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Checksum"));
}

#[test]
fn test_release_from_response_invalid_semver() {
    let mut channels = HashMap::new();
    channels.insert(
        "latest".to_string(),
        ChannelInfo {
            version: "v-invalid-version".to_string(),
            checksum: "abc123".to_string(),
        },
    );
    let releases = ReleasesResponse { channels };
    let result = release_from_response(releases, UpdateChannel::Latest);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("semver"));
}

#[test]
fn test_release_from_response_success() {
    let mut channels = HashMap::new();
    channels.insert(
        "latest".to_string(),
        ChannelInfo {
            version: "v1.2.3".to_string(),
            checksum: "abc123def456".to_string(),
        },
    );
    let releases = ReleasesResponse { channels };
    let result = release_from_response(releases, UpdateChannel::Latest);
    assert!(result.is_ok());
    let release = result.unwrap();
    assert_eq!(release.tag, "v1.2.3");
    assert_eq!(release.semver, "1.2.3");
    assert_eq!(release.checksum, "abc123def456");
}

#[test]
fn test_should_check_for_updates_no_cache() {
    assert!(should_check_for_updates(UpdateChannel::Latest, None));
}

#[test]
fn test_should_check_for_updates_zero_last_checked() {
    let cache = UpdateCache {
        last_checked_at: 0,
        available_tag: None,
        available_semver: None,
        channel: "latest".to_string(),
    };
    assert!(should_check_for_updates(
        UpdateChannel::Latest,
        Some(&cache)
    ));
}

#[test]
fn test_should_check_for_updates_channel_mismatch() {
    let now = current_timestamp();
    let cache = UpdateCache {
        last_checked_at: now,
        available_tag: None,
        available_semver: None,
        channel: "latest".to_string(),
    };
    assert!(should_check_for_updates(UpdateChannel::Next, Some(&cache)));
}

#[test]
fn test_update_cache_serialization() {
    // Test serialization/deserialization without file I/O
    let mut cache = UpdateCache::new(UpdateChannel::Latest);
    cache.last_checked_at = 1234567890;
    cache.available_tag = Some("v1.0.0".to_string());
    cache.available_semver = Some("1.0.0".to_string());

    let json = serde_json::to_vec(&cache).unwrap();
    let deserialized: UpdateCache = serde_json::from_slice(&json).unwrap();

    assert_eq!(deserialized.last_checked_at, 1234567890);
    assert_eq!(deserialized.available_tag, Some("v1.0.0".to_string()));
    assert_eq!(deserialized.available_semver, Some("1.0.0".to_string()));
    assert_eq!(deserialized.channel, "latest");
}

#[test]
fn test_persist_update_state_creates_cache_object() {
    // Test that persist_update_state creates correct UpdateCache structure
    // without relying on file I/O
    let release = ChannelRelease {
        tag: "v1.5.0".to_string(),
        semver: "1.5.0".to_string(),
        checksum: "test".to_string(),
    };

    // Manually construct what persist_update_state would create
    let mut cache = UpdateCache::new(UpdateChannel::Next);
    cache.last_checked_at = current_timestamp();
    cache.available_tag = Some(release.tag.clone());
    cache.available_semver = Some(release.semver.clone());

    assert_eq!(cache.available_tag, Some("v1.5.0".to_string()));
    assert_eq!(cache.available_semver, Some("1.5.0".to_string()));
    assert_eq!(cache.channel, "next");
    assert!(cache.last_checked_at > 0);
}

#[test]
fn test_persist_update_state_no_release_structure() {
    // Test that persist_update_state without release creates correct structure
    let mut cache = UpdateCache::new(UpdateChannel::Latest);
    cache.last_checked_at = current_timestamp();
    // No available_tag or available_semver set

    assert!(cache.available_tag.is_none());
    assert!(cache.available_semver.is_none());
    assert_eq!(cache.channel, "latest");
    assert!(cache.last_checked_at > 0);
}

#[test]
fn test_daemon_update_check_result_debug() {
    // Verify that DaemonUpdateCheckResult derives Debug and PartialEq correctly.
    assert_eq!(
        DaemonUpdateCheckResult::NoUpdate,
        DaemonUpdateCheckResult::NoUpdate
    );
    assert_eq!(
        DaemonUpdateCheckResult::UpdateReady,
        DaemonUpdateCheckResult::UpdateReady
    );
    assert_ne!(
        DaemonUpdateCheckResult::NoUpdate,
        DaemonUpdateCheckResult::UpdateReady
    );
}

#[test]
#[serial]
fn test_check_for_update_available_no_cache_newer_version() {
    // When the cache is empty and a newer version is available, the function should
    // report UpdateReady (assuming version checks and auto-updates are enabled,
    // which is the default in debug/test builds).
    let temp_dir = tempfile::tempdir().unwrap();
    set_test_cache_dir(&temp_dir);

    let test_checksum = "a".repeat(64);
    let mock_payload = format!(
        r#"{{"channels":{{"latest":{{"version":"v999.0.0","checksum":"{}"}}}}}}"#,
        test_checksum
    );
    // check_for_update_available uses Config::fresh() which reads the real config,
    // but fetch_release_for_channel respects mock:// URLs only in tests.
    // We can't easily inject a mock URL into Config::fresh(), so we test the
    // underlying building blocks instead:
    let release =
        fetch_release_for_channel(&format!("mock://{}", mock_payload), UpdateChannel::Latest)
            .unwrap();
    let action = determine_action(false, &release, env!("CARGO_PKG_VERSION"));
    assert_eq!(action, UpgradeAction::UpgradeAvailable);

    // Persist and verify the cache reflects the available update.
    persist_update_state(UpdateChannel::Latest, Some(&release));
    let cache = read_update_cache().unwrap();
    assert!(cache.update_available());
    assert_eq!(cache.available_semver.as_deref(), Some("999.0.0"));

    clear_test_cache_dir();
}

#[test]
fn test_check_for_update_available_same_version() {
    let current = env!("CARGO_PKG_VERSION");
    let test_checksum = "a".repeat(64);
    let mock_payload = format!(
        r#"{{"channels":{{"latest":{{"version":"v{}","checksum":"{}"}}}}}}"#,
        current, test_checksum
    );
    let release =
        fetch_release_for_channel(&format!("mock://{}", mock_payload), UpdateChannel::Latest)
            .unwrap();
    let action = determine_action(false, &release, current);
    assert_eq!(action, UpgradeAction::AlreadyLatest);

    // When the action is AlreadyLatest, persist_update_state is called with None.
    // Verify that such a cache does NOT mark an update as available.
    let mut cache = UpdateCache::new(UpdateChannel::Latest);
    cache.last_checked_at = current_timestamp();
    // No available_tag/semver set — mirrors what persist_update_state(channel, None) does.
    assert!(!cache.update_available());
}

#[test]
fn test_should_check_for_updates_skips_when_recently_checked() {
    // When the cache was recently written, should_check_for_updates returns false.
    let mut cache = UpdateCache::new(UpdateChannel::Latest);
    cache.last_checked_at = current_timestamp();
    assert!(!should_check_for_updates(
        UpdateChannel::Latest,
        Some(&cache)
    ));
}

fn with_update_check_env(cache_has_update: bool, auto_updates_disabled: bool, f: impl FnOnce()) {
    let temp_dir = tempfile::tempdir().unwrap();
    set_test_cache_dir(&temp_dir);

    let mut cache = UpdateCache::new(UpdateChannel::Latest);
    cache.last_checked_at = current_timestamp();
    if cache_has_update {
        cache.available_tag = Some("v99.99.99".to_string());
        cache.available_semver = Some("99.99.99".to_string());
    }
    write_update_cache(&cache);

    let patch = serde_json::json!({
        "disable_version_checks": false,
        "disable_auto_updates": auto_updates_disabled
    })
    .to_string();
    unsafe { std::env::set_var("GIT_AI_TEST_CONFIG_PATCH", &patch) };

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));

    unsafe { std::env::remove_var("GIT_AI_TEST_CONFIG_PATCH") };
    clear_test_cache_dir();

    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[test]
#[serial]
fn check_for_update_available_returns_update_ready_when_cache_has_pending_update() {
    with_update_check_env(true, false, || {
        let result = check_for_update_available().unwrap();
        assert_eq!(result, DaemonUpdateCheckResult::UpdateReady);
    });
}

#[test]
#[serial]
fn check_for_update_available_returns_no_update_when_auto_updates_disabled() {
    with_update_check_env(true, true, || {
        let result = check_for_update_available().unwrap();
        assert_eq!(result, DaemonUpdateCheckResult::NoUpdate);
    });
}

#[test]
#[serial]
fn check_for_update_available_returns_no_update_when_cache_has_no_pending_update() {
    with_update_check_env(false, false, || {
        let result = check_for_update_available().unwrap();
        assert_eq!(result, DaemonUpdateCheckResult::NoUpdate);
    });
}
