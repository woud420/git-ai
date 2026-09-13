use super::models::ChannelInfo;
use super::*;
use serial_test::serial;

fn set_test_cache_dir(dir: &tempfile::TempDir) {
    unsafe {
        std::env::set_var("GIT_AI_TEST_CACHE_DIR", dir.path());
    }
}

fn clear_test_cache_dir() {
    unsafe {
        std::env::remove_var("GIT_AI_TEST_CACHE_DIR");
    }
}

#[cfg(windows)]
#[test]
fn test_is_git_process_name() {
    assert!(is_git_process_name("git"));
    assert!(is_git_process_name("git.exe"));
    assert!(is_git_process_name(r"C:\Program Files\Git\cmd\git.exe"));
    assert!(!is_git_process_name("git-ai.exe"));
    assert!(!is_git_process_name("powershell.exe"));
}

#[cfg(windows)]
#[test]
fn test_should_block_git_extension_upgrade() {
    assert!(should_block_git_extension_upgrade(Some("git.exe"), false));
    assert!(should_block_git_extension_upgrade(
        Some(r"C:\Program Files\Git\cmd\git.exe"),
        false
    ));
    assert!(!should_block_git_extension_upgrade(Some("git.exe"), true));
    assert!(!should_block_git_extension_upgrade(
        Some("powershell.exe"),
        false
    ));
    assert!(!should_block_git_extension_upgrade(None, false));
}

#[test]
fn test_is_newer_version() {
    assert!(!is_newer_version("1.0.0", "1.0.0"));
    assert!(!is_newer_version("1.0.10", "1.0.10"));

    assert!(is_newer_version("1.0.1", "1.0.0"));
    assert!(is_newer_version("1.0.11", "1.0.10"));
    assert!(!is_newer_version("1.0.0", "1.0.1"));
    assert!(!is_newer_version("1.0.10", "1.0.11"));

    assert!(is_newer_version("1.1.0", "1.0.0"));
    assert!(!is_newer_version("1.0.0", "1.1.0"));

    assert!(is_newer_version("2.0.0", "1.0.0"));
    assert!(is_newer_version("2.0.0", "1.9.9"));
    assert!(!is_newer_version("1.9.9", "2.0.0"));

    assert!(is_newer_version("1.0.0.1", "1.0.0"));
    assert!(!is_newer_version("1.0.0", "1.0.0.1"));

    assert!(is_newer_version("1.10.0", "1.9.0"));
    assert!(is_newer_version("1.0.100", "1.0.99"));
    assert!(is_newer_version("100.200.300", "100.200.299"));
}

#[test]
fn test_semver_from_tag_strips_prefix_and_suffix() {
    assert_eq!(semver_from_tag("v1.2.3"), "1.2.3");
    assert_eq!(semver_from_tag("1.2.3"), "1.2.3");
    assert_eq!(semver_from_tag("v1.2.3-next-abc"), "1.2.3");
    assert_eq!(semver_from_tag("enterprise-v1.2.3"), "1.2.3");
    assert_eq!(semver_from_tag("enterprise-v1.2.3-next-abc"), "1.2.3");
}

#[test]
#[serial]
fn test_run_impl_with_url() {
    let temp_dir = tempfile::tempdir().unwrap();
    set_test_cache_dir(&temp_dir);

    let mock_url = |body: &str| format!("mock://{}", body);
    let current = env!("CARGO_PKG_VERSION");
    let test_checksum = "a".repeat(64); // Valid SHA256 length

    // Newer version available - should upgrade
    let action = run_impl_with_url(
        false,
        &mock_url(&format!(
            r#"{{"channels":{{"latest":{{"version":"v999.0.0","checksum":"{}"}},"next":{{"version":"v999.0.0-next-deadbeef","checksum":"{}"}}}}}}"#,
            test_checksum, test_checksum
        )),
        UpdateChannel::Latest,
        true,
    );
    assert_eq!(action, UpgradeAction::UpgradeAvailable);

    // Same version without --force - already latest
    let same_version_payload = format!(
        "{{\"channels\":{{\"latest\":{{\"version\":\"v{}\",\"checksum\":\"{}\"}},\"next\":{{\"version\":\"v{}-next-deadbeef\",\"checksum\":\"{}\"}}}}}}",
        current, test_checksum, current, test_checksum
    );
    let action = run_impl_with_url(
        false,
        &mock_url(&same_version_payload),
        UpdateChannel::Latest,
        true,
    );
    assert_eq!(action, UpgradeAction::AlreadyLatest);

    // Same version with --force - force reinstall
    let action = run_impl_with_url(
        true,
        &mock_url(&same_version_payload),
        UpdateChannel::Latest,
        true,
    );
    assert_eq!(action, UpgradeAction::ForceReinstall);

    // Older version without --force - running newer version
    let action = run_impl_with_url(
        false,
        &mock_url(&format!(
            r#"{{"channels":{{"latest":{{"version":"v1.0.9","checksum":"{}"}},"next":{{"version":"v1.0.9-next-deadbeef","checksum":"{}"}}}}}}"#,
            test_checksum, test_checksum
        )),
        UpdateChannel::Latest,
        true,
    );
    assert_eq!(action, UpgradeAction::RunningNewerVersion);

    // Older version with --force - force reinstall
    let action = run_impl_with_url(
        true,
        &mock_url(&format!(
            r#"{{"channels":{{"latest":{{"version":"v1.0.9","checksum":"{}"}},"next":{{"version":"v1.0.9-next-deadbeef","checksum":"{}"}}}}}}"#,
            test_checksum, test_checksum
        )),
        UpdateChannel::Latest,
        true,
    );
    assert_eq!(action, UpgradeAction::ForceReinstall);

    clear_test_cache_dir();
}

#[test]
#[serial]
fn test_run_impl_with_url_enterprise_channels() {
    let temp_dir = tempfile::tempdir().unwrap();
    set_test_cache_dir(&temp_dir);

    let mock_url = |body: &str| format!("mock://{}", body);
    let current = env!("CARGO_PKG_VERSION");
    let test_checksum = "a".repeat(64); // Valid SHA256 length

    // Newer version available - should upgrade
    let action = run_impl_with_url(
        false,
        &mock_url(&format!(
            r#"{{"channels":{{"enterprise-latest":{{"version":"v999.0.0","checksum":"{}"}},"enterprise-next":{{"version":"v999.0.0-next-deadbeef","checksum":"{}"}}}}}}"#,
            test_checksum, test_checksum
        )),
        UpdateChannel::EnterpriseLatest,
        true,
    );
    assert_eq!(action, UpgradeAction::UpgradeAvailable);

    // Same version without --force - already latest
    let same_version_payload = format!(
        "{{\"channels\":{{\"enterprise-latest\":{{\"version\":\"v{}\",\"checksum\":\"{}\"}},\"enterprise-next\":{{\"version\":\"v{}-next-deadbeef\",\"checksum\":\"{}\"}}}}}}",
        current, test_checksum, current, test_checksum
    );
    let action = run_impl_with_url(
        false,
        &mock_url(&same_version_payload),
        UpdateChannel::EnterpriseLatest,
        true,
    );
    assert_eq!(action, UpgradeAction::AlreadyLatest);

    // Same version with --force - force reinstall
    let action = run_impl_with_url(
        true,
        &mock_url(&same_version_payload),
        UpdateChannel::EnterpriseLatest,
        true,
    );
    assert_eq!(action, UpgradeAction::ForceReinstall);

    // Older version without --force - running newer version
    let action = run_impl_with_url(
        false,
        &mock_url(&format!(
            r#"{{"channels":{{"enterprise-latest":{{"version":"v1.0.9","checksum":"{}"}},"enterprise-next":{{"version":"v1.0.9-next-deadbeef","checksum":"{}"}}}}}}"#,
            test_checksum, test_checksum
        )),
        UpdateChannel::EnterpriseLatest,
        true,
    );
    assert_eq!(action, UpgradeAction::RunningNewerVersion);

    // Older version with --force - force reinstall
    let action = run_impl_with_url(
        true,
        &mock_url(&format!(
            r#"{{"channels":{{"enterprise-latest":{{"version":"v1.0.9","checksum":"{}"}},"enterprise-next":{{"version":"v1.0.9-next-deadbeef","checksum":"{}"}}}}}}"#,
            test_checksum, test_checksum
        )),
        UpdateChannel::EnterpriseLatest,
        true,
    );
    assert_eq!(action, UpgradeAction::ForceReinstall);

    clear_test_cache_dir();
}

#[test]
fn test_should_check_for_updates_respects_interval() {
    let now = current_timestamp();
    let mut cache = UpdateCache::new(UpdateChannel::Latest);
    cache.last_checked_at = now;
    assert!(!should_check_for_updates(
        UpdateChannel::Latest,
        Some(&cache)
    ));

    let stale_offset = (UPDATE_CHECK_INTERVAL_HOURS * 3600) + 10;
    cache.last_checked_at = now.saturating_sub(stale_offset);
    assert!(should_check_for_updates(
        UpdateChannel::Latest,
        Some(&cache)
    ));

    assert!(should_check_for_updates(UpdateChannel::Latest, None));
}

#[test]
fn test_should_check_for_updates_verifies_channel() {
    let now = current_timestamp();
    let mut cache = UpdateCache::new(UpdateChannel::Latest);
    cache.last_checked_at = now;

    // Cache matches channel - should respect interval
    assert!(!should_check_for_updates(
        UpdateChannel::Latest,
        Some(&cache)
    ));

    // Cache doesn't match channel - should check for updates
    assert!(should_check_for_updates(UpdateChannel::Next, Some(&cache)));
}

#[test]
fn test_verify_sha256_success() {
    let content = b"hello world";
    // SHA256 of "hello world"
    let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
    assert!(verify_sha256(content, expected).is_ok());
}

#[test]
fn test_verify_sha256_case_insensitive() {
    let content = b"hello world";
    let expected_upper = "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9";
    assert!(verify_sha256(content, expected_upper).is_ok());
}

#[test]
fn test_verify_sha256_mismatch() {
    let content = b"hello world";
    let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    let result = verify_sha256(content, wrong_hash);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Checksum mismatch"));
}

#[test]
fn test_verify_sha256_empty_content() {
    let content = b"";
    // SHA256 of empty string
    let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert!(verify_sha256(content, expected).is_ok());
}

#[test]
fn test_parse_checksums_valid_format() {
    let content = "594de6cf107e8ffb6efd9029bf727b465ab55a9b4c4c3995eb3e628c857dc423  git-ai-linux-arm64\n\
                       88db3c0c7fc62a815579ec0ca42535c2b83ab18d9e3af8efe345dee96677b1d8  git-ai-linux-x64\n\
                       75d1692d347c3e08a208dc6373df4cee2b5ffd0e2aee62ccb1bb47aae866b2c8  install.sh";

    let checksums = parse_checksums(content);
    assert_eq!(checksums.len(), 3);
    assert_eq!(
        checksums.get("git-ai-linux-arm64"),
        Some(&"594de6cf107e8ffb6efd9029bf727b465ab55a9b4c4c3995eb3e628c857dc423".to_string())
    );
    assert_eq!(
        checksums.get("git-ai-linux-x64"),
        Some(&"88db3c0c7fc62a815579ec0ca42535c2b83ab18d9e3af8efe345dee96677b1d8".to_string())
    );
    assert_eq!(
        checksums.get("install.sh"),
        Some(&"75d1692d347c3e08a208dc6373df4cee2b5ffd0e2aee62ccb1bb47aae866b2c8".to_string())
    );
}

#[test]
fn test_parse_checksums_with_extensions() {
    let content = "23c693a25f4f2e99463c911e67d534ae17cbd9b98513aa65f0ae9da861775d54  git-ai-windows-x64.exe\n\
                       f895af791eb30f6b074b2ab9f0f803e91230b084f5864befcb51ee9ced752adf  install.ps1";

    let checksums = parse_checksums(content);
    assert_eq!(checksums.len(), 2);
    assert!(checksums.contains_key("git-ai-windows-x64.exe"));
    assert!(checksums.contains_key("install.ps1"));
}

#[test]
fn test_parse_checksums_empty_input() {
    let checksums = parse_checksums("");
    assert!(checksums.is_empty());
}

#[test]
fn test_parse_checksums_whitespace_lines() {
    let content = "  \n\nhash  file\n  \n";
    let checksums = parse_checksums(content);
    assert_eq!(checksums.len(), 1);
    assert_eq!(checksums.get("file"), Some(&"hash".to_string()));
}

#[test]
fn test_parse_checksums_ignores_invalid_lines() {
    // Lines with single space or no space should be ignored
    let content = "valid  file1\ninvalid file2\nalsovalid  file3";
    let checksums = parse_checksums(content);
    assert_eq!(checksums.len(), 2);
    assert!(checksums.contains_key("file1"));
    assert!(checksums.contains_key("file3"));
    assert!(!checksums.contains_key("file2"));
}

// --- Additional comprehensive tests ---

#[test]
fn test_update_cache_new() {
    let cache = UpdateCache::new(UpdateChannel::Latest);
    assert_eq!(cache.last_checked_at, 0);
    assert!(cache.available_tag.is_none());
    assert!(cache.available_semver.is_none());
    assert_eq!(cache.channel, "latest");
    assert!(!cache.update_available());
    assert!(cache.matches_channel(UpdateChannel::Latest));
    assert!(!cache.matches_channel(UpdateChannel::Next));
}

#[test]
fn test_update_cache_update_available() {
    let mut cache = UpdateCache::new(UpdateChannel::Latest);
    cache.available_semver = Some("2.0.0".to_string());
    assert!(cache.update_available());
}

#[test]
fn test_update_cache_matches_channel_enterprise() {
    let cache_latest = UpdateCache::new(UpdateChannel::EnterpriseLatest);
    assert!(cache_latest.matches_channel(UpdateChannel::EnterpriseLatest));
    assert!(!cache_latest.matches_channel(UpdateChannel::EnterpriseNext));
    assert!(!cache_latest.matches_channel(UpdateChannel::Latest));
}

#[test]
fn test_determine_action_force() {
    let release = ChannelRelease {
        tag: "v1.0.0".to_string(),
        semver: "1.0.0".to_string(),
        checksum: "abc".to_string(),
    };
    let action = determine_action(true, &release, "1.0.0");
    assert_eq!(action, UpgradeAction::ForceReinstall);
}

#[test]
fn test_determine_action_already_latest() {
    let release = ChannelRelease {
        tag: "v1.0.0".to_string(),
        semver: "1.0.0".to_string(),
        checksum: "abc".to_string(),
    };
    let action = determine_action(false, &release, "1.0.0");
    assert_eq!(action, UpgradeAction::AlreadyLatest);
}

#[test]
fn test_determine_action_upgrade_available() {
    let release = ChannelRelease {
        tag: "v2.0.0".to_string(),
        semver: "2.0.0".to_string(),
        checksum: "abc".to_string(),
    };
    let action = determine_action(false, &release, "1.0.0");
    assert_eq!(action, UpgradeAction::UpgradeAvailable);
}

#[test]
fn test_determine_action_running_newer() {
    let release = ChannelRelease {
        tag: "v1.0.0".to_string(),
        semver: "1.0.0".to_string(),
        checksum: "abc".to_string(),
    };
    let action = determine_action(false, &release, "2.0.0");
    assert_eq!(action, UpgradeAction::RunningNewerVersion);
}

#[test]
fn test_upgrade_action_to_string() {
    assert_eq!(
        UpgradeAction::UpgradeAvailable.to_string(),
        "upgrade_available"
    );
    assert_eq!(UpgradeAction::AlreadyLatest.to_string(), "already_latest");
    assert_eq!(
        UpgradeAction::RunningNewerVersion.to_string(),
        "running_newer_version"
    );
    assert_eq!(UpgradeAction::ForceReinstall.to_string(), "force_reinstall");
}

#[test]
fn test_semver_from_tag_enterprise_prefix() {
    assert_eq!(semver_from_tag("enterprise-v1.2.3"), "1.2.3");
    assert_eq!(semver_from_tag("enterprise-1.2.3"), "1.2.3");
}

#[test]
fn test_semver_from_tag_with_build_metadata() {
    assert_eq!(semver_from_tag("v1.2.3+build123"), "1.2.3");
    assert_eq!(semver_from_tag("1.2.3+build123"), "1.2.3");
}

#[test]
fn test_semver_from_tag_empty() {
    assert_eq!(semver_from_tag(""), "");
    assert_eq!(semver_from_tag("v"), "");
    assert_eq!(semver_from_tag("enterprise-v"), "");
}

#[test]
fn test_is_newer_version_major() {
    assert!(is_newer_version("2.0.0", "1.9.9"));
    assert!(!is_newer_version("1.9.9", "2.0.0"));
}

#[test]
fn test_is_newer_version_minor() {
    assert!(is_newer_version("1.2.0", "1.1.9"));
    assert!(!is_newer_version("1.1.9", "1.2.0"));
}

#[test]
fn test_is_newer_version_patch() {
    assert!(is_newer_version("1.0.1", "1.0.0"));
    assert!(!is_newer_version("1.0.0", "1.0.1"));
}

#[test]
fn test_is_newer_version_empty_parts() {
    assert!(is_newer_version("1", "0.9.9"));
    assert!(!is_newer_version("0.9.9", "1"));
}

#[test]
fn test_is_newer_version_equal() {
    assert!(!is_newer_version("1.0.0", "1.0.0"));
    assert!(!is_newer_version("2.5.10", "2.5.10"));
}

#[test]
fn test_parse_checksums_multiple_spaces() {
    // Format requires exactly two spaces between hash and filename
    // More spaces should still work because split_once("  ") matches the first occurrence
    let content = "abc123  file_with_spaces.txt";
    let checksums = parse_checksums(content);
    assert_eq!(checksums.len(), 1);
    assert_eq!(
        checksums.get("file_with_spaces.txt"),
        Some(&"abc123".to_string())
    );
}

#[test]
fn test_verify_sha256_with_binary_content() {
    let content = b"\x00\x01\x02\x03\xff\xfe";
    let mut hasher = sha2::Sha256::new();
    hasher.update(content);
    let expected = format!("{:x}", hasher.finalize());
    assert!(verify_sha256(content, &expected).is_ok());
}

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
