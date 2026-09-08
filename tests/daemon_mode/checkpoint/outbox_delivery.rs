use super::super::*;

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn checkpoint_empty_allowlist_does_not_publish_outbox() {
    let mut repo = TestRepo::new_dedicated_daemon();
    repo.patch_git_ai_config(|patch| {
        patch.allowed_repositories = Some(Vec::new());
    });
    fs::write(
        repo.path().join("denied-checkpoint.txt"),
        "sensitive edit\n",
    )
    .expect("failed to write denied checkpoint fixture");

    let mut command = repo.git_ai_command_without_pre_sync_for_test(
        &["checkpoint", "mock_ai", "denied-checkpoint.txt"],
        &[],
    );
    let output = command
        .output()
        .expect("failed to invoke checkpoint with an empty allowlist");

    assert!(
        output.status.success(),
        "checkpoint hooks must keep their exit-zero contract: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Skipping checkpoint because no repositories are allowed"),
        "checkpoint should report the existing collection-policy denial: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        ready_checkpoint_outbox_records(&repo).is_empty(),
        "a denied checkpoint must not publish any durable outbox record"
    );
}

#[test]
fn dedicated_daemon_home_config_projects_config_patch_fields() {
    let repo = TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.codex_hooks_format = Some("hooks_json".to_string());
        patch.transcript_streaming_lookback_days = Some(30);
        patch.telemetry_oss_disabled = Some(true);
    });

    let config_path = repo.daemon_home_path().join(".git-ai").join("config.json");
    let config: Value = serde_json::from_slice(
        &fs::read(&config_path).expect("dedicated daemon HOME config should exist"),
    )
    .expect("dedicated daemon HOME config should be valid JSON");

    assert_eq!(
        (
            config.get("codex_hooks_format"),
            config.get("transcript_streaming_lookback_days"),
            config.get("telemetry_oss"),
            config.get("telemetry_oss_disabled"),
        ),
        (
            Some(&json!("hooks_json")),
            Some(&json!(30)),
            Some(&json!("off")),
            None,
        ),
        "the dedicated daemon config should project the ConfigPatch fields, \
         with telemetry_oss_disabled translated to legacy telemetry_oss"
    );
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn checkpoint_transport_failure_publishes_exact_outbox_record() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let file_path = repo.path().join("deferred-checkpoint.txt");
    fs::write(&file_path, "base\n").expect("failed to write base checkpoint fixture");
    repo.git_og(&["add", "deferred-checkpoint.txt"])
        .expect("failed to stage base checkpoint fixture");
    repo.git_og(&["commit", "-m", "base commit"])
        .expect("failed to create base checkpoint commit");
    let base_commit = repo
        .git_og(&["rev-parse", "HEAD"])
        .expect("failed to resolve base checkpoint commit")
        .trim()
        .to_string();

    let edited_content = "base\ncaptured while daemon unavailable\n";
    fs::write(&file_path, edited_content).expect("failed to write deferred checkpoint fixture");

    let mut command = repo.git_ai_command_without_pre_sync_for_test(
        &["checkpoint", "mock_ai", "deferred-checkpoint.txt"],
        &[],
    );
    let output = command
        .output()
        .expect("failed to invoke checkpoint without a daemon");

    assert!(
        output.status.success(),
        "checkpoint hooks must keep their exit-zero contract: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let records = ready_checkpoint_outbox_records(&repo);
    assert_eq!(
        records.len(),
        1,
        "an allowed checkpoint with failed IPC must publish exactly one ready record; records={records:?}, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record_path = &records[0];
    let delivery = decode_delivery(
        &fs::read(record_path).expect("failed to read published checkpoint outbox record"),
    )
    .expect("published checkpoint outbox record should decode");
    let expected_filename =
        ready_filename(&delivery).expect("delivery should produce a safe ready filename");

    assert_eq!(
        record_path.file_name().and_then(|name| name.to_str()),
        Some(expected_filename.as_str())
    );
    delivery
        .validate()
        .expect("published checkpoint delivery should validate");
    assert_eq!(delivery.schema_version, CHECKPOINT_DELIVERY_SCHEMA_VERSION);
    assert_eq!(delivery.batch_ordinal, 0);
    assert!(!delivery.delivery_id.is_empty());
    assert!(!delivery.batch_id.is_empty());
    assert!(delivery.captured_at_unix_ms > 0);
    assert!(!delivery.producer_version.is_empty());

    let request = &delivery.request;
    assert!(!request.trace_id.is_empty());
    assert_eq!(request.checkpoint_kind, CheckpointKind::AiAgent);
    assert_eq!(request.path_role, PreparedPathRole::Edited);
    assert!(request.stream_source.is_none());
    assert_eq!(
        request.metadata.get("edit_kind").map(String::as_str),
        Some("file_edit")
    );
    let agent = request
        .agent_id
        .as_ref()
        .expect("mock_ai checkpoint should retain its agent identity");
    assert_eq!(agent.tool, "mock_ai");
    assert!(agent.id.starts_with("ai-thread-"));
    assert_eq!(agent.model, "unknown");

    assert_eq!(request.files.len(), 1);
    let checkpoint_file = &request.files[0];
    assert_eq!(
        checkpoint_file
            .path
            .canonicalize()
            .expect("captured file path should canonicalize"),
        file_path
            .canonicalize()
            .expect("fixture file path should canonicalize")
    );
    assert_eq!(
        checkpoint_file
            .repo_work_dir
            .canonicalize()
            .expect("captured repository path should canonicalize"),
        repo.canonical_path()
    );
    assert_eq!(checkpoint_file.content.as_deref(), Some(edited_content));
    match &checkpoint_file.base_commit {
        BaseCommit::Sha(sha) => assert_eq!(sha, &base_commit),
        BaseCommit::Initial => panic!("committed fixture should capture its base commit SHA"),
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn sandbox_checkpoint_autostart_publishes_outbox_without_starting_daemon() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let file_path = repo.path().join("sandbox-checkpoint.txt");
    fs::write(&file_path, "base\n").expect("failed to write base checkpoint fixture");
    repo.git_og(&["add", "sandbox-checkpoint.txt"])
        .expect("failed to stage base checkpoint fixture");
    repo.git_og(&["commit", "-m", "base commit"])
        .expect("failed to create base checkpoint commit");
    fs::write(&file_path, "base\ncaptured in sandbox\n")
        .expect("failed to write sandbox checkpoint fixture");

    let mut command = repo.git_ai_command_without_pre_sync_for_test(
        &["checkpoint", "mock_ai", "sandbox-checkpoint.txt"],
        &[],
    );
    let output = command
        .env("GIT_AI_TEST_ALLOW_DAEMON_AUTOSPAWN", "1")
        .env("SANDBOX_RUNTIME", "seatbelt")
        .output()
        .expect("failed to invoke sandbox checkpoint");
    let stderr = String::from_utf8_lossy(&output.stderr);

    if daemon_control_socket_path(&repo).exists() {
        let _ = send_control_request(
            &daemon_control_socket_path(&repo),
            &ControlRequest::Shutdown,
        );
    }

    assert!(
        output.status.success(),
        "sandbox checkpoints must preserve their exit-zero contract: stdout={} stderr={stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        ready_checkpoint_outbox_records(&repo).len(),
        1,
        "sandbox checkpoint should publish exactly one durable outbox record; stderr={stderr}"
    );
    assert!(
        !daemon_control_socket_path(&repo).exists(),
        "sandbox checkpoint must not start a daemon"
    );
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn checkpoint_repository_discovery_failure_does_not_publish_outbox() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let nested_repo = repo.path().join("malformed-nested-repo");
    fs::create_dir_all(&nested_repo).expect("failed to create malformed nested repo fixture");
    fs::write(
        nested_repo.join(".git"),
        "this is not a valid gitdir pointer\n",
    )
    .expect("failed to write malformed nested .git fixture");
    fs::write(nested_repo.join("private-edit.txt"), "sensitive edit\n")
        .expect("failed to write nested checkpoint fixture");

    let mut command = repo.git_ai_command_without_pre_sync_for_test(
        &[
            "checkpoint",
            "mock_ai",
            "malformed-nested-repo/private-edit.txt",
        ],
        &[],
    );
    let output = command
        .output()
        .expect("failed to invoke checkpoint for malformed nested repository");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "repository authorization failures must preserve the hook exit-zero contract: stdout={} stderr={stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        ready_checkpoint_outbox_records(&repo).is_empty(),
        "a checkpoint whose repository cannot be verified must not publish an outbox record"
    );
    assert!(
        stderr
            .contains("Skipping checkpoint because repository authorization could not be verified"),
        "repository discovery failure should produce an actionable redacted warning: stderr={stderr}"
    );
    assert!(
        !stderr.contains(repo.path().to_string_lossy().as_ref())
            && !stderr.contains("private-edit.txt"),
        "repository discovery diagnostics must not expose repository or file paths: stderr={stderr}"
    );
}
