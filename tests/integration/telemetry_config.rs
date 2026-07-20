//! The master `telemetry` switch defaults to off: no Sentry/PostHog, metrics
//! upload, daemon-log upload, or heartbeat egress unless the user enables it.

use crate::repos::test_repo::TestRepo;

#[test]
fn test_telemetry_defaults_to_off() {
    let repo = TestRepo::new();
    let value = repo
        .git_ai(&["config", "telemetry"])
        .expect("telemetry should be readable");
    assert!(
        value.contains("off"),
        "expected telemetry off by default, got: {value}"
    );
}

#[test]
fn test_telemetry_can_be_enabled_and_unset() {
    let repo = TestRepo::new();
    repo.git_ai(&["config", "set", "telemetry", "on"])
        .expect("enabling telemetry should succeed");

    // Read without the pre-invocation config sync, which would rewrite
    // config.json from the test patch and drop the value just set.
    let value = repo
        .git_ai_without_pre_sync_for_test(&["config", "telemetry"])
        .expect("telemetry should be readable");
    assert!(value.contains("on"), "expected on after set, got: {value}");

    repo.git_ai_without_pre_sync_for_test(&["config", "unset", "telemetry"])
        .expect("unsetting telemetry should succeed");
    let value = repo
        .git_ai_without_pre_sync_for_test(&["config", "telemetry"])
        .expect("telemetry should be readable");
    assert!(
        value.contains("off"),
        "expected off after unset, got: {value}"
    );
}

#[test]
fn test_telemetry_rejects_invalid_values() {
    let repo = TestRepo::new();
    let result = repo.git_ai(&["config", "set", "telemetry", "sometimes"]);
    assert!(
        result.is_err() || result.as_deref().unwrap_or("").contains("Invalid"),
        "expected invalid telemetry value to be rejected, got: {result:?}"
    );
}
