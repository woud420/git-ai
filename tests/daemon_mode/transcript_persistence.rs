use super::*;
use git_ai::metrics::types::MetricEventId;
use git_ai::model::repository::streams_db::StreamsDatabase;

#[test]
fn transcript_batch_retries_after_atomic_persistence_failure() {
    let storage = tempfile::tempdir().unwrap();
    let metrics_path = storage.path().join("metrics.db");
    let repo = TestRepo::new_with_daemon_env_and_patch(
        &[(
            "GIT_AI_TEST_METRICS_DB_PATH",
            metrics_path.to_str().unwrap(),
        )],
        |patch| {
            patch.telemetry = Some("off".into());
            patch.feature_flags = Some(json!({"transcript_sweep": false}));
        },
    );
    let file_path = repo.path().join("edited.txt");
    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("seed transcript persistence test")
        .unwrap();
    let mut file = repo.filename("edited.txt");
    file.assert_committed_lines(lines!["base".unattributed_human()]);

    let connection = rusqlite::Connection::open(&metrics_path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_second_transcript_event BEFORE INSERT ON metrics
             WHEN NEW.external_event_id = 'persistence-event-2'
             BEGIN SELECT RAISE(ABORT, 'test transcript insert failure'); END;",
        )
        .unwrap();
    let directory = repo.daemon_home_path().join(".claude/projects/persistence");
    fs::create_dir_all(&directory).unwrap();
    let transcript = directory.join("persistence-session.jsonl");
    let timestamp = chrono::Utc::now().to_rfc3339();
    let mut contents = String::new();
    for index in 1..=2 {
        contents.push_str(
            &json!({
                "type": "user", "sessionId": "persistence-session",
                "uuid": format!("persistence-event-{index}"),
                "parentUuid": if index == 2 { Some("persistence-event-1") } else { None },
                "timestamp": timestamp, "cwd": repo.canonical_path(),
                "message": {"role": "user", "content": format!(
                    "message {index}: sk_test_4eC39HqLyjWDarjtT1zdp7dc"
                )}
            })
            .to_string(),
        );
        contents.push('\n');
    }
    fs::write(&transcript, &contents).unwrap();
    let hook = json!({
        "cwd": repo.canonical_path(), "hook_event_name": "PostToolUse",
        "session_id": "persistence-session", "tool_name": "Edit",
        "transcript_path": transcript,
        "tool_input": {"file_path": file_path}
    })
    .to_string();
    fs::write(&file_path, "base\nAI edit\n").unwrap();
    repo.checkpoint_with_hook_input("claude", &hook).unwrap();
    repo.git_ai(&["await", "--timeout", "30"]).unwrap();

    let streams = StreamsDatabase::open(
        repo.daemon_home_path()
            .join(".git-ai/internal/transcripts-db"),
    )
    .unwrap();
    let stream = || {
        streams
            .all_streams()
            .unwrap()
            .into_iter()
            .find(|row| row.external_session_id == "persistence-session")
            .expect("checkpoint must register the trusted transcript")
    };
    let rows = || {
        connection
            .prepare(
                "SELECT external_event_id, event_json FROM metrics
                 WHERE external_session_id = 'persistence-session' AND event_kind = ?1 ORDER BY id",
            )
            .unwrap()
            .query_map([MetricEventId::SessionEvent as u16], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert!(
        rows().is_empty(),
        "a failed insert must roll back the whole batch"
    );
    assert_eq!(
        stream().watermark_value,
        "0",
        "failed persistence must remain retryable"
    );

    connection
        .execute_batch("DROP TRIGGER reject_second_transcript_event;")
        .unwrap();
    fs::write(&file_path, "base\nAI edit\nAI retry\n").unwrap();
    repo.checkpoint_with_hook_input("claude", &hook).unwrap();
    repo.git_ai(&["await", "--timeout", "30"]).unwrap();
    let persisted = rows();
    assert_eq!(
        persisted.len(),
        2,
        "retry must retain every event exactly once"
    );
    for (index, (event_id, event_json)) in persisted.iter().enumerate() {
        assert_eq!(event_id, &format!("persistence-event-{}", index + 1));
        assert!(event_json.contains(&format!("message {}", index + 1)));
        assert!(!event_json.contains("sk_test_4eC39HqLyjWDarjtT1zdp7dc"));
        assert!(event_json.contains("********"));
    }
    assert_eq!(stream().watermark_value, contents.len().to_string());
    repo.stage_all_and_commit("commit after transcript persistence recovery")
        .unwrap();
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "AI edit".ai(),
        "AI retry".ai()
    ]);
}
