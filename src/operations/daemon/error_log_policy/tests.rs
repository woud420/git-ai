use super::*;

fn git_error(stderr: &str, arg: &str) -> GitAiError {
    GitAiError::GitCliError {
        code: Some(128),
        stderr: stderr.to_string(),
        args: vec![arg.to_string()],
    }
}

#[test]
fn identical_failures_keep_three_errors_then_rate_limit_summaries() {
    let mut policy = ErrorLogPolicy::default();
    let now = Instant::now();
    let error = git_error("broken git config", "status");
    for _ in 0..3 {
        assert_eq!(
            policy.failure("family", "command", &error, now),
            Decision::Error
        );
    }
    assert_eq!(
        policy.failure("family", "command", &error, now),
        Decision::Summary(1)
    );
    assert_eq!(
        policy.failure("family", "command", &error, now),
        Decision::Debug
    );
    assert_eq!(
        policy.failure(
            "family",
            "command",
            &error,
            now + SUMMARY_INTERVAL - Duration::from_secs(1)
        ),
        Decision::Debug
    );
    assert_eq!(
        policy.failure("family", "command", &error, now + SUMMARY_INTERVAL),
        Decision::Summary(3)
    );
}

#[test]
fn changed_failure_success_and_other_families_have_fresh_error_budgets() {
    let mut policy = ErrorLogPolicy::default();
    let now = Instant::now();
    let error = git_error("broken config", "status");
    for _ in 0..4 {
        policy.failure("family", "command", &error, now);
    }
    assert_eq!(
        policy.failure("other", "command", &error, now),
        Decision::Error
    );
    assert_eq!(
        policy.failure("family", "checkpoint", &error, now),
        Decision::Error
    );
    assert_eq!(
        policy.failure(
            "family",
            "checkpoint",
            &git_error("access denied", "status"),
            now
        ),
        Decision::Error
    );
    policy.success("family");
    assert_eq!(
        policy.failure("family", "command", &error, now),
        Decision::Error
    );
}

#[test]
fn fingerprint_ignores_git_argv_and_full_object_ids_but_preserves_other_errors() {
    let first = git_error(&format!("missing object {}", "a".repeat(40)), "one");
    let second = git_error(&format!("missing object {}", "b".repeat(40)), "two");
    assert_eq!(
        fingerprint("command", &first),
        fingerprint("command", &second)
    );
    assert_ne!(
        fingerprint("command", &git_error("limit 1234567", "one")),
        fingerprint("command", &git_error("limit 7654321", "one"))
    );
    assert_ne!(
        fingerprint("command", &first),
        fingerprint("checkpoint", &first)
    );
    let mut different_code = first.clone();
    if let GitAiError::GitCliError { code, .. } = &mut different_code {
        *code = Some(1);
    }
    assert_ne!(
        fingerprint("command", &first),
        fingerprint("command", &different_code)
    );
}

#[test]
fn state_is_bounded_and_idle_families_receive_a_fresh_budget() {
    let mut policy = ErrorLogPolicy::default();
    let now = Instant::now();
    let error = git_error("broken config", "status");
    for i in 0..MAX_FAMILIES + 10 {
        policy.failure(
            &format!("family-{i}"),
            "command",
            &error,
            now + Duration::from_millis(i as u64),
        );
    }
    assert_eq!(policy.families.len(), MAX_FAMILIES);
    assert!(!policy.families.contains_key("family-0"));
    assert_eq!(
        policy.failure(
            "fresh",
            "command",
            &error,
            now + IDLE_TIMEOUT + Duration::from_secs(60)
        ),
        Decision::Error
    );
    assert_eq!(policy.families.len(), 1);
}

#[test]
fn deleted_repository_errors_are_expected_but_existing_or_unrelated_errors_are_not() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("gone.git");
    let error = git_error("fatal: not a git repository", "status");
    assert!(expected_condition(missing.to_str().unwrap(), &error));
    assert!(!expected_condition(root.path().to_str().unwrap(), &error));
    assert!(!expected_condition(
        missing.to_str().unwrap(),
        &git_error("fatal: access denied", "status")
    ));
    let permission = Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "denied",
    ));
    assert!(!definitely_missing(permission));
}

#[test]
fn discovery_classifier_matches_the_actual_no_exec_error() {
    let outside = tempfile::tempdir().unwrap();
    let error =
        crate::operations::git::repository::discover_repository_in_path_no_git_exec(outside.path())
            .err()
            .unwrap();
    assert!(is_discovery_miss(&error), "{error}");
    assert!(!is_discovery_miss(&git_error(
        "fatal: not a git repository",
        "status"
    )));
}

#[tokio::test]
async fn expected_conditions_preserve_status_without_allocating_suppression_state() {
    let coordinator = ActorDaemonCoordinator::new();
    let root = tempfile::tempdir().unwrap();
    let family = root.path().join("deleted.git");
    let family = family.to_str().unwrap();
    let error = git_error("fatal: not a git repository", "status");
    let original = error.to_string();
    coordinator.record_and_log_side_effect_result::<()>(
        family,
        4,
        "command_apply",
        "command apply failed",
        &Err(error),
    );
    assert_eq!(
        coordinator.latest_side_effect_error(family).unwrap(),
        Some(original)
    );
    assert!(
        coordinator
            .error_log_policy
            .lock()
            .unwrap()
            .families
            .is_empty()
    );
}
