use super::fixtures::FIXTURES;
use super::report::{BenchSettings, Measurement, write_report};
use crate::repos::test_repo::{TestRepo, get_binary_path};
use git_ai::model::checkpoint_request::BaseCommit;
use git_ai::operations::daemon::{ControlRequest, DaemonConfig, send_control_request_with_timeout};
use serde_json::{Value, json};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
#[ignore = "explicit TestRepo socket benchmark; prebuilt daemon override supported"]
fn benchmark_checkpoint_ipc_socket_ack() {
    let settings = BenchSettings::from_env();
    let mut results = Vec::new();
    for fixture in FIXTURES {
        let repo = TestRepo::new_dedicated_daemon();
        let root = repo.canonical_path();
        let base = fixture.delivery(&root, 0);
        for file in &base.request.files {
            std::fs::write(&file.path, file.content.as_ref().unwrap()).unwrap();
        }
        let base_sha = repo
            .stage_all_and_commit("IPC benchmark base")
            .unwrap()
            .commit_sha;
        repo.sync_daemon();
        let working_log = repo.current_working_logs();
        let socket = DaemonConfig::from_home(repo.test_home_path()).control_socket_path;
        let mut durations = Vec::with_capacity(settings.samples);
        let mut measured_ids = Vec::with_capacity(settings.samples);
        let mut latest = base;
        for iteration in 1..=settings.socket_warmups + settings.samples {
            let mut delivery = fixture.delivery(&root, iteration);
            delivery.captured_at_unix_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()
                .try_into()
                .unwrap();
            for file in &mut delivery.request.files {
                file.base_commit = BaseCommit::Sha(base_sha.clone());
                std::fs::write(&file.path, file.content.as_ref().unwrap()).unwrap();
            }
            let request = ControlRequest::CheckpointDeliver {
                delivery: Box::new(delivery.clone()),
            };
            let started = Instant::now();
            let response =
                send_control_request_with_timeout(&socket, &request, Duration::from_secs(30))
                    .expect("checkpoint transport should acknowledge");
            let elapsed = started.elapsed();
            assert!(response.ok, "checkpoint rejected: {:?}", response.error);
            if iteration > settings.socket_warmups {
                durations.push(elapsed);
                measured_ids.push(delivery.delivery_id.clone());
            }
            latest = delivery;
        }
        // ACK must mean each measured delivery has already reached working-log storage.
        let checkpoints = working_log.read_all_checkpoints().unwrap();
        for id in &measured_ids {
            assert_eq!(
                checkpoints
                    .iter()
                    .filter(|checkpoint| checkpoint.delivery_id.as_ref() == Some(id))
                    .count(),
                1,
                "each acknowledged delivery must be applied exactly once"
            );
        }
        let applied = checkpoints
            .iter()
            .find(|checkpoint| checkpoint.delivery_id.as_ref() == Some(&latest.delivery_id))
            .unwrap();
        assert_eq!(applied.entries.len(), latest.request.files.len());
        for file in &latest.request.files {
            let relative_path = file.path.strip_prefix(&root).unwrap().to_string_lossy();
            let entry = applied
                .entries
                .iter()
                .find(|entry| entry.file == relative_path)
                .unwrap();
            assert_eq!(
                working_log.get_file_version(&entry.blob_sha).unwrap(),
                *file.content.as_ref().unwrap()
            );
        }
        results.push(json!({
            "fixture": fixture.name,
            "files": fixture.files,
            "content_bytes": fixture.content_bytes(),
            "transport": "production_control_sender",
            "ack": Measurement::from_durations(&durations),
            "measured_deliveries_applied_once": measured_ids.len(),
            "latest_content_verified": true,
        }));
    }
    write_report(
        "socket_ack",
        &settings,
        Value::Array(results),
        Some(get_binary_path()),
    );
}
