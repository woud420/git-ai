use super::*;
use git_ai::metrics::types::MetricEventId;
use git_ai::model::repository::streams_db::StreamsDatabase;

struct TranscriptFixture {
    repo: TestRepo,
    storage: tempfile::TempDir,
    file_path: PathBuf,
    hook: String,
    offsets: Vec<usize>,
}

impl TranscriptFixture {
    fn new(line_limit: &str, batch_limit: &str, messages: &[&str]) -> Self {
        let storage = tempfile::tempdir().unwrap();
        let metrics_path = storage.path().join("metrics.db");
        let claude_config = storage.path().join("claude");
        let repo = TestRepo::new_with_daemon_env_and_patch(
            &[
                (
                    "GIT_AI_TEST_METRICS_DB_PATH",
                    metrics_path.to_str().unwrap(),
                ),
                ("CLAUDE_CONFIG_DIR", claude_config.to_str().unwrap()),
                ("GIT_AI_MAX_TRANSCRIPT_LINE_BYTES", line_limit),
                ("GIT_AI_MAX_TRANSCRIPT_BATCH_BYTES", batch_limit),
            ],
            |patch| {
                patch.telemetry = Some("off".into());
                patch.feature_flags = Some(json!({"transcript_sweep": false}));
            },
        );
        let file_path = repo.path().join("edited.txt");
        fs::write(&file_path, "base\n").unwrap();
        repo.stage_all_and_commit("base").unwrap();
        repo.filename("edited.txt")
            .assert_committed_lines(lines!["base".unattributed_human()]);
        let directory = claude_config.join("projects/budget");
        fs::create_dir_all(&directory).unwrap();
        let transcript = directory.join("budget-session.jsonl");
        let timestamp = chrono::Utc::now().to_rfc3339();
        let mut contents = String::new();
        let mut offsets = Vec::new();
        for (index, content) in messages.iter().enumerate() {
            contents.push_str(&format!(
                "{}\n",
                json!({
                    "type": "user", "sessionId": "budget-session", "uuid": format!("budget-{index}"),
                    "timestamp": timestamp, "cwd": repo.canonical_path(),
                    "message": {"role": "user", "content": content},
                }),
            ));
            offsets.push(contents.len());
        }
        fs::write(&transcript, &contents).unwrap();
        let hook = json!({
            "cwd": repo.canonical_path(), "hook_event_name": "PostToolUse",
            "session_id": "budget-session", "tool_name": "Edit",
            "transcript_path": transcript, "tool_input": {"file_path": file_path},
        })
        .to_string();
        Self {
            repo,
            storage,
            file_path,
            hook,
            offsets,
        }
    }

    fn checkpoint(&self, text: &str) {
        fs::write(&self.file_path, text).unwrap();
        self.repo
            .git_ai_with_env(
                &["checkpoint", "claude", "--hook-input", &self.hook],
                &[(
                    "CLAUDE_CONFIG_DIR",
                    self.storage.path().join("claude").to_str().unwrap(),
                )],
            )
            .unwrap();
        self.repo.git_ai(&["await", "--timeout", "30"]).unwrap();
    }

    fn assert_ingested(&self, count: usize) {
        let connection = git_ai::model::repository::sqlite::open_with_memory_limits(
            self.storage.path().join("metrics.db"),
        )
        .unwrap();
        let actual = connection.query_row(
            "SELECT COUNT(*) FROM metrics WHERE external_session_id = 'budget-session' AND event_kind = ?1",
            [MetricEventId::SessionEvent as u16], |row| row.get::<_, usize>(0),
        ).unwrap();
        assert_eq!(
            actual, count,
            "every accepted transcript event must be stored exactly once"
        );
        let streams = StreamsDatabase::open(
            self.repo
                .daemon_home_path()
                .join(".git-ai/internal/transcripts-db"),
        )
        .unwrap();
        let watermark = streams
            .all_streams()
            .unwrap()
            .into_iter()
            .find(|row| row.external_session_id == "budget-session")
            .expect("trusted transcript must be registered")
            .watermark_value;
        assert_eq!(watermark, self.offsets[count - 1].to_string());
    }

    fn restart(&mut self, line_limit: &str, batch_limit: &str) {
        let metrics_path = self.storage.path().join("metrics.db");
        let claude_config = self.storage.path().join("claude");
        self.repo.restart_dedicated_daemon_with_env_for_test(&[
            (
                "GIT_AI_TEST_METRICS_DB_PATH",
                metrics_path.to_str().unwrap(),
            ),
            ("CLAUDE_CONFIG_DIR", claude_config.to_str().unwrap()),
            ("GIT_AI_MAX_TRANSCRIPT_LINE_BYTES", line_limit),
            ("GIT_AI_MAX_TRANSCRIPT_BATCH_BYTES", batch_limit),
        ]);
    }

    fn assert_committed(&self) {
        self.repo
            .stage_all_and_commit("commit after bounded transcript processing")
            .unwrap();
        self.repo
            .filename("edited.txt")
            .assert_committed_lines(lines![
                "base".unattributed_human(),
                "AI edit".ai(),
                "AI retry".ai(),
                "AI resumed".ai(),
            ]);
    }
}

#[test]
fn oversized_transcript_preserves_its_cursor_and_resumes_after_budget_increase() {
    let mut fixture = TranscriptFixture::new(
        "1024",
        "8192",
        &["first message", &"x".repeat(4096), "last message"],
    );
    fixture.checkpoint("base\nAI edit\n");
    fixture.checkpoint("base\nAI edit\nAI retry\n");
    fixture.assert_ingested(1);
    fixture.restart("8192", "8192");
    fixture.checkpoint("base\nAI edit\nAI retry\nAI resumed\n");
    fixture.assert_ingested(3);
    fixture.assert_committed();
}

#[test]
fn transcript_batch_budget_persists_complete_records_before_reading_the_next_batch() {
    let message = "x".repeat(400);
    let mut fixture = TranscriptFixture::new("8192", "1024", &[&message, &message, &message]);
    assert!(fixture.offsets[0] < 1024 && fixture.offsets[1] > 1024);
    let hook: serde_json::Value = serde_json::from_str(&fixture.hook).unwrap();
    let transcript = PathBuf::from(hook["transcript_path"].as_str().unwrap());
    let prefix = "\n".repeat(2048);
    fs::write(
        &transcript,
        prefix.clone() + &fs::read_to_string(&transcript).unwrap(),
    )
    .unwrap();
    for offset in &mut fixture.offsets {
        *offset += prefix.len();
    }
    let connection = git_ai::model::repository::sqlite::open_with_memory_limits(
        fixture
            .repo
            .daemon_home_path()
            .join(".git-ai/internal/transcripts-db"),
    )
    .unwrap();
    connection.execute_batch(
        "CREATE TABLE observed_watermarks (value TEXT NOT NULL);
         CREATE TRIGGER observe_batch AFTER UPDATE OF watermark_value ON tracked_streams
         WHEN NEW.external_session_id = 'budget-session' AND NEW.watermark_value != OLD.watermark_value
         BEGIN INSERT INTO observed_watermarks VALUES (NEW.watermark_value); END;"
    ).unwrap();
    fixture.checkpoint("base\nAI edit\nAI retry\nAI resumed\n");
    fixture.assert_ingested(3);
    let mut statement = connection
        .prepare("SELECT value FROM observed_watermarks ORDER BY rowid")
        .unwrap();
    let offsets: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        offsets,
        fixture
            .offsets
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
    );
    fixture.assert_committed();
}
