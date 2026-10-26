use crate::test_utils::DurationStatistics;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Serialize)]
pub(super) struct BenchSettings {
    pub(super) samples: usize,
    #[serde(rename = "codec_warmup_ms", with = "duration_millis")]
    pub(super) codec_warmup: Duration,
    #[serde(rename = "sample_time_ms", with = "duration_millis")]
    pub(super) sample_time: Duration,
    pub(super) socket_warmups: usize,
}

impl BenchSettings {
    pub(super) fn from_env() -> Self {
        Self {
            samples: setting("SAMPLES", 30, 3, 1000),
            codec_warmup: Duration::from_millis(setting("WARMUP_MS", 100, 1, 10_000) as u64),
            sample_time: Duration::from_millis(setting("SAMPLE_MS", 10, 1, 1000) as u64),
            socket_warmups: setting("SOCKET_WARMUPS", 5, 1, 100),
        }
    }
}

fn setting(name: &str, default: usize, min: usize, max: usize) -> usize {
    let key = format!("GIT_AI_CHECKPOINT_IPC_BENCH_{name}");
    let value = std::env::var(&key)
        .map(|value| {
            value
                .parse::<usize>()
                .expect("benchmark setting is an integer")
        })
        .unwrap_or(default);
    assert!((min..=max).contains(&value), "{key} must be {min}..={max}");
    value
}

mod duration_millis {
    use serde::Serializer;
    use std::time::Duration;

    pub(super) fn serialize<S: Serializer>(
        duration: &Duration,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_u128(duration.as_millis())
    }
}

#[derive(Clone, Copy)]
pub(super) struct Batch {
    elapsed: Duration,
    iterations: u32,
}

pub(super) fn measure_batch(target: Duration, mut operation: impl FnMut()) -> Batch {
    let start = Instant::now();
    let mut iterations = 0_u32;
    loop {
        operation();
        iterations += 1;
        if start.elapsed() >= target {
            return Batch {
                elapsed: start.elapsed(),
                iterations,
            };
        }
    }
}

#[derive(Serialize)]
pub(super) struct Measurement {
    samples: usize,
    iterations_per_sample: Vec<u32>,
    ns_per_operation: Vec<f64>,
    p50_ns: f64,
    p95_ns: f64,
    min_ns: f64,
    max_ns: f64,
}

impl Measurement {
    pub(super) fn from_batches(batches: &[Batch]) -> Self {
        let durations: Vec<_> = batches
            .iter()
            .map(|batch| batch.elapsed / batch.iterations)
            .collect();
        let stats = DurationStatistics::from_durations(&durations);
        let ns = |duration: Duration| duration.as_secs_f64() * 1e9;
        Self {
            samples: batches.len(),
            iterations_per_sample: batches.iter().map(|batch| batch.iterations).collect(),
            ns_per_operation: durations.iter().copied().map(ns).collect(),
            p50_ns: ns(stats.percentile_nearest_rank(0.50).unwrap()),
            p95_ns: ns(stats.percentile_nearest_rank(0.95).unwrap()),
            min_ns: ns(stats.min().unwrap()),
            max_ns: ns(stats.max().unwrap()),
        }
    }

    pub(super) fn from_durations(durations: &[Duration]) -> Self {
        Self::from_batches(
            &durations
                .iter()
                .map(|elapsed| Batch {
                    elapsed: *elapsed,
                    iterations: 1,
                })
                .collect::<Vec<_>>(),
        )
    }
}

fn command_output(arguments: &[&str]) -> String {
    let output = Command::new(arguments[0])
        .args(&arguments[1..])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("read benchmark provenance");
    assert!(
        output.status.success(),
        "benchmark provenance command failed"
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn file_digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn binary_version(path: &Path) -> String {
    let output = Command::new(path).arg("--version").output().unwrap();
    assert!(output.status.success(), "daemon version probe failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn benchmark_sources_digest() -> String {
    let sources = [
        (
            "tests/integration/checkpoint_ipc_benchmark.rs",
            include_str!("../checkpoint_ipc_benchmark.rs"),
        ),
        (
            "tests/integration/checkpoint_ipc_benchmark/codecs.rs",
            include_str!("codecs.rs"),
        ),
        (
            "tests/integration/checkpoint_ipc_benchmark/fixtures.rs",
            include_str!("fixtures.rs"),
        ),
        (
            "tests/integration/checkpoint_ipc_benchmark/report.rs",
            include_str!("report.rs"),
        ),
        (
            "tests/integration/checkpoint_ipc_benchmark/socket.rs",
            include_str!("socket.rs"),
        ),
    ];
    let mut digest = Sha256::new();
    for (path, source) in sources {
        digest.update((path.len() as u64).to_le_bytes());
        digest.update(path.as_bytes());
        digest.update((source.len() as u64).to_le_bytes());
        digest.update(source.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

pub(super) fn write_report(
    kind: &str,
    settings: &BenchSettings,
    results: Value,
    daemon_binary: Option<&Path>,
) {
    let executable = std::env::current_exe().unwrap();
    let diff = command_output(&["git", "diff", "HEAD", "--binary"]);
    let daemon_version = daemon_binary.map(binary_version);
    let daemon_override = std::env::var_os("GIT_AI_TEST_BINARY_PATH").is_some();
    let report = json!({
        "schema": "checkpoint-ipc-benchmark-v1",
        "kind": kind,
        "settings": settings,
        "provenance": {
            "source_commit": command_output(&["git", "rev-parse", "HEAD"]),
            "source_dirty": !command_output(&["git", "status", "--porcelain"]).is_empty(),
            "tracked_diff_sha256": format!("{:x}", Sha256::digest(diff.as_bytes())),
            "benchmark_sources_sha256": benchmark_sources_digest(),
            "runtime_rustc": command_output(&["rustc", "--version"]),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "test_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "test_executable_sha256": file_digest(&executable),
            "daemon_binary_sha256": daemon_binary.map(file_digest),
            "daemon_binary_path": daemon_binary.map(|path| path.canonicalize().unwrap()),
            "daemon_version": daemon_version,
            "daemon_debug_assertions_observed": daemon_version.as_ref().map(|version| version.ends_with(" (debug)")),
            "daemon_binary_selection": daemon_binary.map(|_| if daemon_override {
                "GIT_AI_TEST_BINARY_PATH override; bind profile to external build provenance"
            } else {
                "debug build (TestRepo default)"
            }),
        },
        "contracts": {
            "codec": "validated delivery payload; decode and roundtrip construct owned Rust DTOs; no control framing",
            "allocation_and_rss_measured": false,
            "percentile": "nearest-rank over per-operation batch means",
            "socket_ack": "direct control sender through live checkpoint application; CLI startup excluded; storage oracle outside samples",
            "decision_release_end_to_end": false,
        },
        "results": results,
    });
    let encoded = serde_json::to_string_pretty(&report).unwrap();
    if let Some(path) = std::env::var_os("GIT_AI_CHECKPOINT_IPC_BENCH_OUTPUT") {
        std::fs::write(path, &encoded).expect("persist checkpoint IPC benchmark report");
    }
    println!(
        "CHECKPOINT_IPC_BENCHMARK_JSON={}",
        serde_json::to_string(&report).unwrap()
    );
}
