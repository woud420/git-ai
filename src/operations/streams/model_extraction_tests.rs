use super::*;
use std::path::{Path, PathBuf};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn create_copilot_otel_db(path: &Path) -> rusqlite::Connection {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let conn = crate::model::repository::sqlite::open_with_memory_limits(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE spans (
                span_id TEXT PRIMARY KEY,
                chat_session_id TEXT,
                request_model TEXT,
                response_model TEXT,
                end_time_ms REAL NOT NULL
            );",
    )
    .unwrap();
    conn
}

fn insert_copilot_otel_model(
    conn: &rusqlite::Connection,
    span_id: &str,
    chat_session_id: &str,
    request_model: Option<&str>,
    response_model: Option<&str>,
    end_time_ms: f64,
) {
    conn.execute(
        "INSERT INTO spans (span_id, chat_session_id, request_model, response_model, end_time_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            span_id,
            chat_session_id,
            request_model,
            response_model,
            end_time_ms
        ],
    )
    .unwrap();
}

fn create_copilot_vscode_workspace() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let user_dir = dir.path().join("User");
    let transcript_path = user_dir
        .join("workspaceStorage")
        .join("workspace-1")
        .join("GitHub.copilot-chat")
        .join("transcripts")
        .join("session-abc.jsonl");
    std::fs::create_dir_all(transcript_path.parent().unwrap()).unwrap();
    std::fs::write(
        &transcript_path,
        r#"{"type":"session.start","data":{"sessionId":"session-abc"}}"#,
    )
    .unwrap();

    let models_path = user_dir
        .join("workspaceStorage")
        .join("workspace-1")
        .join("GitHub.copilot-chat")
        .join("debug-logs")
        .join("session-abc")
        .join("models.json");
    std::fs::create_dir_all(models_path.parent().unwrap()).unwrap();
    std::fs::write(
        &models_path,
        r#"[
                {"id":"claude-sonnet-4","is_chat_default":false},
                {"id":"gpt-4.1","is_chat_default":true}
            ]"#,
    )
    .unwrap();

    let otel_db_path = user_dir
        .join("globalStorage")
        .join("github.copilot-chat")
        .join("agent-traces.db");

    (dir, transcript_path, otel_db_path)
}

#[test]
fn test_extract_model_claude() {
    let path = fixture_path("example-claude-code.jsonl");
    let result = extract_model(&path, StreamFormat::ClaudeJsonl, None).unwrap();
    assert_eq!(result, Some("claude-sonnet-4-20250514".to_string()));
}

#[test]
fn test_extract_model_droid_settings() {
    let path = fixture_path("droid-session.settings.json");
    let result = extract_model_from_droid_settings(&path).unwrap();
    assert_eq!(result, Some("custom:BYOK-GPT-5-MINI-0".to_string()));
}

#[test]
fn test_extract_model_copilot_session() {
    let path = fixture_path("copilot_session_simple.json");
    let result = extract_model(&path, StreamFormat::CopilotSessionJson, None).unwrap();
    assert_eq!(result, Some("copilot/claude-sonnet-4".to_string()));
}

#[test]
fn test_extract_model_copilot_event_stream() {
    let path = fixture_path("copilot_session_event_stream.jsonl");
    let result = extract_model(&path, StreamFormat::CopilotEventStreamJsonl, None).unwrap();
    // No model field in this fixture
    assert_eq!(result, None);
}

#[test]
fn test_extract_model_gemini() {
    let path = fixture_path("gemini-session-simple.jsonl");
    let result = extract_model(&path, StreamFormat::GeminiJsonl, None).unwrap();
    assert_eq!(result, Some("gemini-2.5-flash".to_string()));
}

#[test]
fn test_extract_model_amp() {
    let path = fixture_path("amp-threads/T-019ca1ce-3ae2-7686-a41e-ccc078837f8a.json");
    let result = extract_model(&path, StreamFormat::AmpThreadJson, None).unwrap();
    assert_eq!(result, Some("claude-opus-4-6".to_string()));
}

#[test]
fn test_extract_model_opencode() {
    let path = fixture_path("opencode-sqlite/opencode.db");
    let result = extract_model(
        &path,
        StreamFormat::OpenCodeSqlite,
        Some("test-session-123"),
    )
    .unwrap();
    assert_eq!(result, Some("gpt-5".to_string()));
}

#[test]
fn test_extract_model_opencode_assistant_message_format() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("opencode.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    conn.execute_batch(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
             INSERT INTO message VALUES ('msg-1', 'sess-1', 1000, 1000, '{\"role\":\"assistant\",\"modelID\":\"claude-opus-4-6\",\"providerID\":\"anthropic\"}');",
        ).unwrap();
    drop(conn);

    let result = extract_model(&db_path, StreamFormat::OpenCodeSqlite, Some("sess-1")).unwrap();
    assert_eq!(result, Some("claude-opus-4-6".to_string()));
}

#[test]
fn test_extract_model_copilot_cli() {
    let path = fixture_path("copilot_cli_session_events.jsonl");
    let result = extract_model(&path, StreamFormat::CopilotEventStreamJsonl, None).unwrap();
    assert_eq!(result, Some("gpt-4.1".to_string()));
}

#[test]
fn test_extract_model_copilot_cli_no_model() {
    let path = fixture_path("copilot_cli_session_no_model.jsonl");
    let result = extract_model(&path, StreamFormat::CopilotEventStreamJsonl, None).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_extract_model_missing_file() {
    let path = PathBuf::from("/nonexistent/path/to/file.jsonl");
    let result = extract_model(&path, StreamFormat::ClaudeJsonl, None).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_extract_model_empty_file() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let result = extract_model(file.path(), StreamFormat::ClaudeJsonl, None).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_extract_model_droid_settings_missing_file() {
    let path = PathBuf::from("/nonexistent/settings.json");
    let result = extract_model_from_droid_settings(&path).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_extract_model_unsupported_format_returns_none() {
    let path = fixture_path("example-claude-code.jsonl");
    let result = extract_model(&path, StreamFormat::DroidJsonl, None).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_extract_model_claude_model_not_on_last_line() {
    let path = fixture_path("claude-model-not-last.jsonl");
    let result = extract_model(&path, StreamFormat::ClaudeJsonl, None).unwrap();
    assert_eq!(result, Some("claude-opus-4-6".to_string()));
}

#[test]
fn test_extract_model_skips_synthetic_model() {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, r#"{{"type":"user","message":{{"content":"hello"}}}}"#).unwrap();
    writeln!(file, r#"{{"type":"assistant","message":{{"model":"claude-opus-4-6","content":[{{"type":"text","text":"hi"}}]}}}}"#).unwrap();
    writeln!(file, r#"{{"type":"assistant","message":{{"model":"<synthetic>","content":[{{"type":"text","text":"bye"}}]}}}}"#).unwrap();
    file.flush().unwrap();

    let result = extract_model(file.path(), StreamFormat::ClaudeJsonl, None).unwrap();
    assert_eq!(result, Some("claude-opus-4-6".to_string()));
}

#[test]
fn test_extract_model_copilot_vscode_models_json() {
    let path = fixture_path(
        "copilot_vscode_workspace/GitHub.copilot-chat/transcripts/test-session-abc.jsonl",
    );
    let result = extract_model_from_copilot_models_json(&path).unwrap();
    assert_eq!(result, Some("gpt-4.1".to_string()));
}

#[test]
fn test_extract_model_copilot_vscode_models_json_missing() {
    let path = PathBuf::from("/nonexistent/transcripts/fake-session.jsonl");
    let result = extract_model_from_copilot_models_json(&path).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_extract_model_copilot_otel_newest_request_model_wins() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("agent-traces.db");
    let conn = create_copilot_otel_db(&db_path);
    insert_copilot_otel_model(
        &conn,
        "span-1",
        "session-abc",
        Some("gpt-4.1"),
        Some("gpt-4.1-2025-04-14"),
        1000.0,
    );
    insert_copilot_otel_model(
        &conn,
        "span-2",
        "session-abc",
        Some("claude-sonnet-4"),
        Some("claude-sonnet-4-20250514"),
        2000.0,
    );
    insert_copilot_otel_model(
        &conn,
        "span-3",
        "session-abc",
        None,
        Some("response-only-newer"),
        3000.0,
    );
    insert_copilot_otel_model(
        &conn,
        "span-4",
        "other-session",
        Some("gpt-5"),
        Some("gpt-5-2026-01-01"),
        4000.0,
    );
    drop(conn);

    let result = extract_model(
        &db_path,
        StreamFormat::CopilotOtelSqlite,
        Some("session-abc"),
    )
    .unwrap();
    assert_eq!(result, Some("claude-sonnet-4".to_string()));
}

#[test]
fn test_extract_model_copilot_otel_falls_back_to_response_model() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("agent-traces.db");
    let conn = create_copilot_otel_db(&db_path);
    insert_copilot_otel_model(
        &conn,
        "span-1",
        "session-abc",
        None,
        Some("gpt-4.1-2025-04-14"),
        1000.0,
    );
    insert_copilot_otel_model(
        &conn,
        "span-2",
        "session-abc",
        None,
        Some("gpt-5-2026-01-01"),
        2000.0,
    );
    drop(conn);

    let result = extract_model(
        &db_path,
        StreamFormat::CopilotOtelSqlite,
        Some("session-abc"),
    )
    .unwrap();
    assert_eq!(result, Some("gpt-5-2026-01-01".to_string()));
}

#[test]
fn test_extract_model_copilot_vscode_transcript_prefers_otel_over_models_json() {
    let (_dir, transcript_path, otel_db_path) = create_copilot_vscode_workspace();
    let conn = create_copilot_otel_db(&otel_db_path);
    insert_copilot_otel_model(
        &conn,
        "span-1",
        "session-abc",
        Some("claude-sonnet-4"),
        Some("claude-sonnet-4-20250514"),
        1000.0,
    );
    drop(conn);

    let result = extract_model_from_copilot_vscode_transcript(
        &transcript_path,
        StreamFormat::CopilotEventStreamJsonl,
        "session-abc",
    )
    .unwrap();
    assert_eq!(result, Some("claude-sonnet-4".to_string()));
}

#[test]
fn test_extract_model_copilot_vscode_transcript_falls_back_to_models_json() {
    let (_dir, transcript_path, _otel_db_path) = create_copilot_vscode_workspace();

    let result = extract_model_from_copilot_vscode_transcript(
        &transcript_path,
        StreamFormat::CopilotEventStreamJsonl,
        "session-abc",
    )
    .unwrap();
    assert_eq!(result, Some("gpt-4.1".to_string()));
}

#[test]
fn test_extract_model_head_fallback_for_large_file() {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::with_suffix(".jsonl").unwrap();
    // model_change at the start
    writeln!(file, r#"{{"type":"session.start","data":{{"sessionId":"s1"}},"id":"e1","timestamp":"2026-01-01T00:00:00Z","parentId":null}}"#).unwrap();
    writeln!(file, r#"{{"type":"session.model_change","data":{{"newModel":"gpt-4.1"}},"id":"e2","timestamp":"2026-01-01T00:00:01Z","parentId":"e1"}}"#).unwrap();
    // Pad with >50KB of filler events so the model_change falls outside the tail window
    for i in 0..600 {
        writeln!(file, r#"{{"type":"user.message","data":{{"content":"padding message number {} with extra text to make the line longer and push past the fifty kilobyte tail read window boundary"}},"id":"pad-{}","timestamp":"2026-01-01T00:01:{:02}Z","parentId":null}}"#, i, i, i % 60).unwrap();
    }
    file.flush().unwrap();

    let size = std::fs::metadata(file.path()).unwrap().len();
    assert!(
        size > 51200,
        "file must exceed 50KB tail window, got {}",
        size
    );

    let result = extract_model(file.path(), StreamFormat::CopilotEventStreamJsonl, None).unwrap();
    assert_eq!(result, Some("gpt-4.1".to_string()));
}
