use super::fixtures::FIXTURES;
use super::report::{BenchSettings, Measurement, write_report};
use crate::repos::test_repo::{TestRepo, get_binary_path};
use git_ai::model::checkpoint_delivery::CheckpointDelivery;
use git_ai::model::checkpoint_request::BaseCommit;
use git_ai::operations::daemon::{
    CheckpointTransport, DaemonConfig, send_checkpoint_delivery_with_transport,
};
use git_ai::operations::git::repo_storage::PersistedWorkingLog;
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[test]
#[ignore = "explicit paired socket benchmark; requires a prebuilt daemon override"]
fn benchmark_checkpoint_ipc_socket_ack() {
    assert!(
        std::env::var_os("GIT_AI_TEST_BINARY_PATH").is_some(),
        "set GIT_AI_TEST_BINARY_PATH to the exact prebuilt benchmark daemon"
    );
    let settings = BenchSettings::from_env();
    let transports = [CheckpointTransport::Json, CheckpointTransport::Capnp];
    let names = ["json", "capnp"];
    let mut results = Vec::new();
    for fixture in FIXTURES {
        let repos = [
            TestRepo::new_dedicated_daemon(),
            TestRepo::new_dedicated_daemon(),
        ];
        let roots = repos.each_ref().map(TestRepo::canonical_path);
        let base_shas: [String; 2] = std::array::from_fn(|index| {
            for file in fixture.delivery(&roots[index], 0).request.files {
                std::fs::write(&file.path, file.content.as_ref().unwrap()).unwrap();
            }
            repos[index]
                .stage_all_and_commit("IPC benchmark base")
                .unwrap()
                .commit_sha
        });
        let working_logs = repos.each_ref().map(TestRepo::current_working_logs);
        let sockets = repos
            .each_ref()
            .map(|repo| DaemonConfig::from_home(repo.test_home_path()).control_socket_path);
        let mut durations = [Vec::new(), Vec::new()];
        let mut orders = Vec::new();
        for iteration in 1..=settings.socket_warmups + settings.samples {
            let captured_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()
                .try_into()
                .unwrap();
            let deliveries: [CheckpointDelivery; 2] = std::array::from_fn(|index| {
                let mut delivery = fixture.delivery(&roots[index], iteration);
                delivery.delivery_id.push_str(&format!("-{index}"));
                delivery.batch_id.push_str(&format!("-{index}"));
                delivery.captured_at_unix_ms = captured_at;
                for file in &mut delivery.request.files {
                    file.base_commit = BaseCommit::Sha(base_shas[index].clone());
                    std::fs::write(&file.path, file.content.as_ref().unwrap()).unwrap();
                }
                delivery
            });
            let order = if iteration % 2 == 0 { [0, 1] } else { [1, 0] };
            for index in order {
                let started = Instant::now();
                let response = send_checkpoint_delivery_with_transport(
                    &sockets[index],
                    &deliveries[index],
                    transports[index],
                )
                .expect("checkpoint transport should acknowledge");
                let elapsed = started.elapsed();
                assert!(response.ok, "checkpoint rejected: {:?}", response.error);
                // The pre-captured handle performs no daemon synchronization.
                assert_applied(&working_logs[index], &roots[index], &deliveries[index]);
                if iteration > settings.socket_warmups {
                    durations[index].push(elapsed);
                }
            }
            if iteration > settings.socket_warmups {
                orders.push(order.map(|index| names[index]));
            }
        }
        let ratios: Vec<_> = durations[1]
            .iter()
            .zip(&durations[0])
            .map(|(capnp, json)| capnp.as_secs_f64() / json.as_secs_f64())
            .collect();
        let differences: Vec<_> = durations[1]
            .iter()
            .zip(&durations[0])
            .map(|(capnp, json)| (capnp.as_secs_f64() - json.as_secs_f64()) * 1e9)
            .collect();
        let mut sorted_ratios = ratios.clone();
        sorted_ratios.sort_by(f64::total_cmp);
        results.push(json!({
            "fixture": fixture.name,
            "files": fixture.files,
            "content_bytes": fixture.content_bytes(),
            "pairing": "alternating order; identical content/depth; separate matched daemons; same executable",
            "pair_order": orders,
            "capnp_includes_negotiation": true,
            "json": { "ack": Measurement::from_durations(&durations[0]) },
            "capnp": { "ack": Measurement::from_durations(&durations[1]) },
            "paired_capnp_over_json": ratios,
            "paired_difference_ns": differences,
            "paired_ratio_nearest_rank_p50": sorted_ratios[(sorted_ratios.len() - 1) / 2],
            "post_ack_oracles_verified_per_transport": settings.samples,
            "captured_content_verified_after_every_ack": true,
        }));
    }
    write_report(
        "socket_ack_paired",
        &settings,
        Value::Array(results),
        Some(get_binary_path()),
    );
}

fn assert_applied(working_log: &PersistedWorkingLog, root: &Path, delivery: &CheckpointDelivery) {
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    let applied: Vec<_> = checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.delivery_id.as_ref() == Some(&delivery.delivery_id))
        .collect();
    assert_eq!(
        applied.len(),
        1,
        "ACK must mean this delivery was applied exactly once"
    );
    assert_eq!(applied[0].entries.len(), delivery.request.files.len());
    for file in &delivery.request.files {
        let relative_path = file.path.strip_prefix(root).unwrap().to_string_lossy();
        let entry = applied[0]
            .entries
            .iter()
            .find(|entry| entry.file == relative_path)
            .unwrap();
        assert_eq!(
            working_log.get_file_version(&entry.blob_sha).unwrap(),
            *file.content.as_ref().unwrap()
        );
    }
}
