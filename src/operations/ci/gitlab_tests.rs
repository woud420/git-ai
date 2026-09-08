use super::*;

#[test]
fn test_gitlab_merge_request_deserialization() {
    let json = r#"{
            "iid": 42,
            "title": "Fix bug",
            "source_branch": "feature/fix",
            "target_branch": "main",
            "sha": "abc123",
            "merge_commit_sha": "def456",
            "squash_commit_sha": null,
            "squash": false,
            "source_project_id": 123,
            "target_project_id": 456
        }"#;
    let mr: GitLabMergeRequest = serde_json::from_str(json).unwrap();
    assert_eq!(mr.iid, 42);
    assert_eq!(mr.title, Some("Fix bug".to_string()));
    assert_eq!(mr.source_branch, "feature/fix");
    assert_eq!(mr.target_branch, "main");
    assert_eq!(mr.sha, "abc123");
    assert_eq!(mr.merge_commit_sha, Some("def456".to_string()));
    assert!(mr.squash_commit_sha.is_none());
    assert_eq!(mr.squash, Some(false));
    assert_eq!(mr.source_project_id, 123);
    assert_eq!(mr.target_project_id, 456);
}

#[test]
fn test_gitlab_merge_request_deserialization_with_squash() {
    let json = r#"{
            "iid": 99,
            "title": "Squash merge",
            "source_branch": "feature/squash",
            "target_branch": "main",
            "sha": "head123",
            "merge_commit_sha": "merge456",
            "squash_commit_sha": "squash789",
            "squash": true,
            "source_project_id": 123,
            "target_project_id": 123
        }"#;
    let mr: GitLabMergeRequest = serde_json::from_str(json).unwrap();
    assert_eq!(mr.iid, 99);
    assert_eq!(mr.squash_commit_sha, Some("squash789".to_string()));
    assert_eq!(mr.squash, Some(true));
    assert_eq!(mr.source_project_id, 123);
    assert_eq!(mr.target_project_id, 123);
}

#[test]
fn test_gitlab_merge_request_deserialization_minimal() {
    let json = r#"{
            "iid": 1,
            "source_branch": "dev",
            "target_branch": "main",
            "sha": "abc",
            "source_project_id": 999,
            "target_project_id": 999
        }"#;
    let mr: GitLabMergeRequest = serde_json::from_str(json).unwrap();
    assert_eq!(mr.iid, 1);
    assert!(mr.title.is_none());
    assert!(mr.merge_commit_sha.is_none());
    assert!(mr.squash_commit_sha.is_none());
    assert!(mr.squash.is_none());
    assert_eq!(mr.source_project_id, 999);
    assert_eq!(mr.target_project_id, 999);
}

#[test]
fn test_gitlab_ci_template_yaml_not_empty() {
    assert!(
        !GITLAB_CI_TEMPLATE_YAML.is_empty(),
        "GitLab CI template YAML should not be empty"
    );
}

#[test]
#[serial_test::serial]
fn test_lookback_minutes_defaults_to_15() {
    unsafe { std::env::remove_var("GIT_AI_CI_LOOKBACK_MINUTES") };
    let lookback = std::env::var("GIT_AI_CI_LOOKBACK_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15i64);
    assert_eq!(lookback, 15);
}

#[test]
#[serial_test::serial]
fn test_lookback_minutes_reads_env_var() {
    unsafe { std::env::set_var("GIT_AI_CI_LOOKBACK_MINUTES", "4320") };
    let lookback = std::env::var("GIT_AI_CI_LOOKBACK_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15i64);
    unsafe { std::env::remove_var("GIT_AI_CI_LOOKBACK_MINUTES") };
    assert_eq!(lookback, 4320);
}

#[test]
#[serial_test::serial]
fn test_lookback_minutes_falls_back_on_invalid_value() {
    unsafe { std::env::set_var("GIT_AI_CI_LOOKBACK_MINUTES", "not-a-number") };
    let lookback = std::env::var("GIT_AI_CI_LOOKBACK_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15i64);
    unsafe { std::env::remove_var("GIT_AI_CI_LOOKBACK_MINUTES") };
    assert_eq!(lookback, 15);
}

// ---- CiEvent::Merge.base_sha derivation from diff_refs ----
//
// We prefer `diff_refs.start_sha` (target tip at diff render, semantically
// equal to GitHub's `pull_request.base.sha`) over `diff_refs.base_sha`
// (merge-base). See the `GitLabDiffRefs` docstring for why; tests below
// pin the preference + fallback + every silently-absorbed failure mode.

#[test]
fn test_diff_refs_deserialization_happy() {
    let json = r#"{
            "iid": 42,
            "diff_refs": {
                "base_sha": "0000000000000000000000000000000000000000",
                "head_sha": "1111111111111111111111111111111111111111",
                "start_sha": "2222222222222222222222222222222222222222"
            }
        }"#;
    let details: GitLabMergeRequestDetails = serde_json::from_str(json).unwrap();
    let diff_refs = details.diff_refs.unwrap();
    assert_eq!(
        diff_refs.base_sha,
        Some("0000000000000000000000000000000000000000".to_string())
    );
    assert_eq!(
        diff_refs.start_sha,
        Some("2222222222222222222222222222222222222222".to_string())
    );
}

#[test]
fn test_diff_refs_deserialization_missing_diff_refs() {
    // GitLab notes diff_refs "is empty when the merge request is created,
    // and populates asynchronously"; absorb that as None.
    let json = r#"{"iid": 1}"#;
    let details: GitLabMergeRequestDetails = serde_json::from_str(json).unwrap();
    assert!(details.diff_refs.is_none());
}

#[test]
fn test_diff_refs_deserialization_null_shas() {
    // Both SHAs JSON-null — surface as None on both fields.
    let json = r#"{
            "iid": 7,
            "diff_refs": { "base_sha": null, "start_sha": null }
        }"#;
    let details: GitLabMergeRequestDetails = serde_json::from_str(json).unwrap();
    let diff_refs = details.diff_refs.unwrap();
    assert!(diff_refs.base_sha.is_none());
    assert!(diff_refs.start_sha.is_none());
}

/// Happy path: both SHAs present, we MUST pick start_sha. This is the
/// load-bearing test — picking base_sha here recreates the original
/// #1473 bug for GitLab squash MRs.
#[test]
fn test_fetch_mr_base_sha_prefers_start_sha_over_base_sha() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/projects/123/merge_requests/42")
        .match_header("PRIVATE-TOKEN", "test-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        // Distinct values so the assertion can't pass by coincidence.
        .with_body(
            r#"{
                "iid": 42,
                "diff_refs": {
                    "base_sha":  "0000000000000000000000000000000000000000",
                    "start_sha": "2222222222222222222222222222222222222222",
                    "head_sha":  "1111111111111111111111111111111111111111"
                }
            }"#,
        )
        .create();

    let result = fetch_mr_base_sha(&server.url(), "PRIVATE-TOKEN", "test-token", "123", 42);

    mock.assert();
    assert_eq!(
        result,
        Some("2222222222222222222222222222222222222222".to_string()),
        "must prefer start_sha (target tip) over base_sha (merge-base)"
    );
}

/// Fallback: GitLab returns diff_refs with base_sha populated but
/// start_sha null/missing. Use base_sha and continue; the retain
/// filter is weakened but not broken.
#[test]
fn test_fetch_mr_base_sha_falls_back_to_base_sha_when_start_sha_missing() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/projects/123/merge_requests/42")
        .with_status(200)
        .with_body(
            r#"{
                "iid": 42,
                "diff_refs": {
                    "base_sha":  "0000000000000000000000000000000000000000",
                    "start_sha": null
                }
            }"#,
        )
        .create();

    let result = fetch_mr_base_sha(&server.url(), "PRIVATE-TOKEN", "tok", "123", 42);
    mock.assert();
    assert_eq!(
        result,
        Some("0000000000000000000000000000000000000000".to_string())
    );
}

#[test]
fn test_fetch_mr_base_sha_returns_none_when_both_shas_missing() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/projects/123/merge_requests/42")
        .with_status(200)
        .with_body(
            r#"{
                "iid": 42,
                "diff_refs": { "base_sha": null, "start_sha": null }
            }"#,
        )
        .create();

    let result = fetch_mr_base_sha(&server.url(), "PRIVATE-TOKEN", "tok", "123", 42);
    mock.assert();
    assert!(result.is_none());
}

#[test]
fn test_fetch_mr_base_sha_404_returns_none() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/projects/123/merge_requests/42")
        .with_status(404)
        .with_body(r#"{"message": "404 Not Found"}"#)
        .create();

    let result = fetch_mr_base_sha(&server.url(), "PRIVATE-TOKEN", "tok", "123", 42);
    mock.assert();
    assert!(
        result.is_none(),
        "404 should fall through to None (caller uses empty string)"
    );
}

#[test]
fn test_fetch_mr_base_sha_malformed_body_returns_none() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/projects/123/merge_requests/42")
        .with_status(200)
        .with_body("not json")
        .create();

    let result = fetch_mr_base_sha(&server.url(), "PRIVATE-TOKEN", "tok", "123", 42);
    mock.assert();
    assert!(result.is_none(), "non-JSON body should not panic");
}

#[test]
fn test_fetch_mr_base_sha_missing_diff_refs_returns_none() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/projects/123/merge_requests/42")
        .with_status(200)
        .with_body(r#"{"iid": 42}"#)
        .create();

    let result = fetch_mr_base_sha(&server.url(), "PRIVATE-TOKEN", "tok", "123", 42);
    mock.assert();
    assert!(result.is_none());
}

#[test]
fn test_fetch_mr_base_sha_with_job_token_header() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/projects/123/merge_requests/42")
        .match_header("JOB-TOKEN", "ci-job-token-value")
        .with_status(200)
        // start_sha present so the happy path through JOB-TOKEN auth fires.
        .with_body(r#"{"diff_refs": {"start_sha": "abc1234567890abcdef1234567890abcdef12345"}}"#)
        .create();

    let result = fetch_mr_base_sha(&server.url(), "JOB-TOKEN", "ci-job-token-value", "123", 42);
    mock.assert();
    assert_eq!(
        result,
        Some("abc1234567890abcdef1234567890abcdef12345".to_string())
    );
}
