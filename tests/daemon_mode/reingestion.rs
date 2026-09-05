use super::*;
use git_ai::metrics::{EventAttributes, MetricEvent, PosEncoded, SessionEventValues};
use git_ai::model::repository::metrics_db::MetricsDatabase;

#[test]
fn reingest_command_is_retry_and_restart_safe_without_duplicating_local_history() {
    let mut mock_api = MockApiServer::start();
    let metrics_db_dir = tempfile::tempdir().expect("reingest metrics temp directory");
    let metrics_db_path = metrics_db_dir.path().join("metrics.db");
    let mut repo = TestRepo::new_with_daemon_env_and_patch(
        &[
            ("GIT_AI_API_BASE_URL", mock_api.base_url()),
            ("GIT_AI_API_KEY", "test-api-key"),
            (
                "GIT_AI_TEST_METRICS_DB_PATH",
                metrics_db_path.to_str().unwrap(),
            ),
        ],
        |patch| {
            patch.telemetry = Some("off".to_string());
            patch.telemetry_oss_disabled = Some(true);
        },
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;
    let event = |timestamp: u32, marker: &str| {
        MetricEvent::with_timestamp(
            timestamp,
            &SessionEventValues::new(json!({ "marker": marker })),
            EventAttributes::with_version("test")
                .session_id(marker)
                .trace_id(marker)
                .to_sparse(),
        )
    };
    let serialized = [
        event(now - 300, "before-window"),
        event(now - 200, "inside-window"),
        event(now - 100, "after-window"),
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<Vec<_>, _>>()
    .unwrap();
    MetricsDatabase::open_at_path(&metrics_db_path)
        .unwrap()
        .insert_events_with_delivered_ts(&serialized, Some(u64::from(now)))
        .unwrap();

    let format_time = |timestamp| {
        chrono::DateTime::<chrono::Utc>::from_timestamp(i64::from(timestamp), 0)
            .unwrap()
            .to_rfc3339()
    };
    let output = repo
        .git_ai(&[
            "reingest",
            "--from",
            &format_time(now - 250),
            "--to",
            &format_time(now - 150),
        ])
        .expect("bounded reingestion should succeed");
    assert!(output.contains("reset 1 metric event(s)"), "{output}");
    let retry_output = repo
        .git_ai(&[
            "reingest",
            "--from",
            &format_time(now - 250),
            "--to",
            &format_time(now - 150),
        ])
        .expect("retrying bounded reingestion should succeed");
    assert!(
        retry_output.contains("reset 1 metric event(s)"),
        "{retry_output}"
    );

    let pending_status = MetricsDatabase::open_at_path(&metrics_db_path)
        .unwrap()
        .status()
        .unwrap();
    assert_eq!(pending_status.total, 3);
    assert_eq!(pending_status.pending_retryable, 1);

    repo.patch_git_ai_config(|patch| {
        patch.telemetry = Some("on".to_string());
    });
    repo.restart_dedicated_daemon_with_env_for_test(&[
        ("GIT_AI_API_BASE_URL", mock_api.base_url()),
        ("GIT_AI_API_KEY", "test-api-key"),
        (
            "GIT_AI_TEST_METRICS_DB_PATH",
            metrics_db_path.to_str().unwrap(),
        ),
    ]);
    repo.git_ai(&["await", "--timeout", "30"])
        .expect("restarted daemon should deliver the bounded reingestion");

    let uploads = serde_json::to_string(&mock_api.collect_requests()).unwrap();
    let status = MetricsDatabase::open_at_path(&metrics_db_path)
        .unwrap()
        .status()
        .unwrap();
    assert!(
        uploads.contains("inside-window"),
        "expected in-range upload; requests={uploads}; status={status:?}"
    );
    assert!(!uploads.contains("before-window"));
    assert!(!uploads.contains("after-window"));

    assert_eq!(status.total, 3);
    assert_eq!(status.delivered, 3);
}
