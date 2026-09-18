use super::*;
use git_ai::metrics::types::MetricEventId;
use git_ai::model::repository::streams_db::StreamsDatabase;

const SESSION: &str = "sqlite-budget";

#[test]
fn sqlite_transcript_keeps_its_cursor_until_the_budget_is_raised() {
    let storage = tempfile::tempdir().unwrap();
    let metrics_path = storage.path().join("metrics.db");
    let threads = storage.path().join("opencode");
    fs::create_dir(&threads).unwrap();
    let mut repo = TestRepo::new_with_daemon_env_and_patch(
        &[
            (
                "GIT_AI_TEST_METRICS_DB_PATH",
                metrics_path.to_str().unwrap(),
            ),
            ("GIT_AI_OPENCODE_STORAGE_PATH", threads.to_str().unwrap()),
            ("GIT_AI_MAX_TRANSCRIPT_LINE_BYTES", "1024"),
        ],
        |patch| {
            patch.telemetry = Some("off".into());
            patch.feature_flags = Some(json!({"transcript_sweep": false}));
        },
    );
    fs::write(repo.path().join("edited.txt"), "base\n").unwrap();
    repo.stage_all_and_commit("Base").unwrap();
    repo.filename("edited.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    let db =
        git_ai::model::repository::sqlite::open_with_memory_limits(threads.join("opencode.db"))
            .unwrap();
    db.execute_batch("CREATE TABLE session (id TEXT PRIMARY KEY, parent_id TEXT);
        CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
        CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);").unwrap();
    db.execute("INSERT INTO session VALUES (?1, NULL)", [SESSION])
        .unwrap();
    db.execute(
        "INSERT INTO message VALUES ('message-1', ?1, 1000, 1000, '{\"role\":\"user\"}')",
        [SESSION],
    )
    .unwrap();
    db.execute(
        "INSERT INTO part VALUES ('part-1', 'message-1', ?1, 1000, 1000, ?2)",
        rusqlite::params![
            SESSION,
            json!({"type":"text", "text":"界".repeat(1400)}).to_string()
        ],
    )
    .unwrap();
    let hook = json!({
        "hook_event_name":"PostToolUse", "tool_use_id":"budget-edit",
        "session_id":SESSION, "cwd":repo.canonical_path(),
        "tool_input":{"filePath":repo.path().join("edited.txt")},
    })
    .to_string();
    let checkpoint = |repo: &TestRepo, contents: &str, limit: &str| {
        repo.git_ai(&["checkpoint", "human", "edited.txt"]).unwrap();
        fs::write(repo.path().join("edited.txt"), contents).unwrap();
        repo.git_ai_with_env(
            &["checkpoint", "opencode", "--hook-input", &hook],
            &[
                ("GIT_AI_OPENCODE_STORAGE_PATH", threads.to_str().unwrap()),
                ("GIT_AI_MAX_TRANSCRIPT_LINE_BYTES", limit),
            ],
        )
        .unwrap();
        repo.git_ai(&["await", "--timeout", "30"]).unwrap();
    };
    checkpoint(&repo, "base\nAI edit\n", "1024");
    assert_saved_progress(&repo, &metrics_path, 0);
    repo.restart_dedicated_daemon_with_env_for_test(&[
        (
            "GIT_AI_TEST_METRICS_DB_PATH",
            metrics_path.to_str().unwrap(),
        ),
        ("GIT_AI_OPENCODE_STORAGE_PATH", threads.to_str().unwrap()),
        ("GIT_AI_MAX_TRANSCRIPT_LINE_BYTES", "8192"),
    ]);
    checkpoint(&repo, "base\nAI edit\nAI resumed\n", "8192");
    assert_saved_progress(&repo, &metrics_path, 1);
    repo.stage_all_and_commit("Commit after transcript retry")
        .unwrap();
    repo.filename("edited.txt").assert_committed_lines(lines![
        "base".unattributed_human(),
        "AI edit".ai(),
        "AI resumed".ai(),
    ]);
}

fn assert_saved_progress(repo: &TestRepo, metrics_path: &std::path::Path, count: usize) {
    let db = git_ai::model::repository::sqlite::open_with_memory_limits(metrics_path).unwrap();
    let actual: usize = db
        .query_row(
            "SELECT COUNT(*) FROM metrics WHERE external_session_id = ?1 AND event_kind = ?2",
            rusqlite::params![SESSION, MetricEventId::SessionEvent as u16],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(actual, count, "accepted events must be stored exactly once");
    let streams = StreamsDatabase::open(
        repo.daemon_home_path()
            .join(".git-ai/internal/transcripts-db"),
    )
    .unwrap();
    let row = streams
        .all_streams()
        .unwrap()
        .into_iter()
        .find(|row| row.external_session_id == SESSION)
        .expect("transcript is registered");
    let cursor: git_ai::model::stream_watermark::TimestampWatermark =
        row.watermark_value.parse().unwrap();
    assert_eq!(
        cursor.0.timestamp_millis(),
        if count == 0 { 0 } else { 1000 }
    );
}
