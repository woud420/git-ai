use super::*;
use git_ai::model::repository::metrics_db::MetricsDatabase;

const LIMIT: &str = "8192";

#[test]
fn daemon_metrics_uploads_bound_each_dequeue_before_parsing() {
    check_flush_budget(true);
}

#[test]
fn manual_metrics_uploads_bound_each_dequeue_before_parsing() {
    check_flush_budget(false);
}

fn check_flush_budget(daemon: bool) {
    let storage = tempfile::tempdir().unwrap();
    let db_path = storage.path().join("metrics.db");
    let mut api = MockApiServer::start();
    let base_url = api.base_url().to_string();
    let env = [
        ("GIT_AI_API_BASE_URL", base_url.as_str()),
        ("GIT_AI_API_KEY", "test-api-key"),
        ("GIT_AI_TEST_METRICS_DB_PATH", db_path.to_str().unwrap()),
        ("GIT_AI_MAX_METRICS_FLUSH_CHUNK_BYTES", LIMIT),
    ];
    let mut repo = if daemon {
        TestRepo::new_with_daemon_env_and_patch(&env, |patch| {
            patch.telemetry = Some("on".into());
            patch.telemetry_oss_disabled = Some(true);
        })
    } else {
        TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon)
    };
    repo.patch_git_ai_config(|patch| {
        patch.telemetry = Some("on".into());
        patch.telemetry_oss_disabled = Some(true);
    });
    if daemon {
        fs::write(repo.path().join("base.txt"), "base\n").unwrap();
        repo.stage_all_and_commit("Baseline").unwrap();
        repo.filename("base.txt")
            .assert_committed_lines(lines!["base".unattributed_human()]);
        repo.git_ai(&["await", "--timeout", "30"]).unwrap();
    }
    api.collect_requests();
    let mut db = MetricsDatabase::open_at_path(&db_path).unwrap();
    db.insert_events(&(0..3).map(|id| event(id, 1700)).collect::<Vec<_>>())
        .unwrap();
    if daemon {
        repo.git_ai(&["await", "--timeout", "30"]).unwrap();
    } else {
        repo.git_ai_with_env(&["flush-metrics-db"], &env).unwrap();
    }
    let mut received = Vec::new();
    for request in api.collect_requests() {
        if request["path"] != "/worker/metrics/upload" {
            continue;
        }
        let matching = request["body"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["v"]["99"].as_u64())
            .collect::<Vec<_>>();
        assert!(
            matching.len() <= 1,
            "one upload inflated {} queued payloads above the byte budget",
            matching.len()
        );
        received.extend(matching);
    }
    received.sort_unstable();
    assert_eq!(
        received,
        [0, 1, 2],
        "all queued payloads must be delivered exactly once"
    );
    assert_eq!(db.status().unwrap().processing, 0);
}

#[test]
fn oversized_metric_stays_unclaimed_until_a_larger_budget_is_used() {
    let storage = tempfile::tempdir().unwrap();
    let db_path = storage.path().join("metrics.db");
    let mut api = MockApiServer::start();
    let base_url = api.base_url().to_string();
    let mut repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    repo.patch_git_ai_config(|patch| {
        patch.telemetry = Some("on".into());
        patch.telemetry_oss_disabled = Some(true);
    });
    let mut db = MetricsDatabase::open_at_path(&db_path).unwrap();
    db.insert_events(&[event(7, 5000)]).unwrap();
    let mut env = [
        ("GIT_AI_API_BASE_URL", base_url.as_str()),
        ("GIT_AI_API_KEY", "test-api-key"),
        ("GIT_AI_TEST_METRICS_DB_PATH", db_path.to_str().unwrap()),
        ("GIT_AI_MAX_METRICS_FLUSH_CHUNK_BYTES", LIMIT),
    ];
    repo.git_ai_with_env(&["flush-metrics-db"], &env).unwrap();
    let status = db.status().unwrap();
    assert_eq!(status.pending_retryable, 1);
    assert_eq!(status.processing, 0);
    assert_eq!(status.delivered, 0);
    assert!(
        api.collect_requests()
            .iter()
            .all(|r| r["path"] != "/worker/metrics/upload")
    );
    env[3].1 = "32768";
    repo.git_ai_with_env(&["flush-metrics-db"], &env).unwrap();
    let status = db.status().unwrap();
    assert_eq!(status.pending_retryable, 0);
    assert_eq!(status.processing, 0);
    assert_eq!(status.delivered, 1);
}

fn event(id: u64, characters: usize) -> String {
    json!({"t": git_ai::model::clock::now_secs(), "e": 1, "v": {"98": "界".repeat(characters), "99":id}, "a":{}}).to_string()
}
