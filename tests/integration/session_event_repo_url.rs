use crate::repos::test_repo::TestRepo;
use git_ai::metrics::{EventAttributes, MetricEvent, SessionEventValues};
use git_ai::model::stream_watermark::{ByteOffsetWatermark, WatermarkType};
use git_ai::operations::streams::agents::ClaudeAgent;
use git_ai::operations::streams::sweep::StreamFormat;
use git_ai::operations::streams::{StreamRecord, StreamsDatabase};
use git_ai::repo_url::resolve_repo_url_from_path;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// === Helper functions ===

fn write_claude_transcript_with_cwd(path: &Path, cwd: &str) {
    let events = [
        json!({"type":"user","message":{"content":"hello","role":"user"},
               "uuid":"u1","parentUuid":null,"cwd":cwd,"sessionId":"test-sess",
               "timestamp":"2026-05-01T00:00:00Z"}),
        json!({"type":"assistant","message":{"model":"claude-opus-4-6","id":"msg1","role":"assistant",
               "content":[{"type":"text","text":"hi"}],"stop_reason":"end_turn",
               "usage":{"input_tokens":1,"output_tokens":1}},
               "uuid":"a1","parentUuid":"u1","cwd":cwd,"sessionId":"test-sess",
               "timestamp":"2026-05-01T00:00:01Z"}),
    ];
    let content: String = events.iter().map(|e| format!("{}\n", e)).collect();
    fs::write(path, content).unwrap();
}

fn write_claude_transcript_without_cwd(path: &Path) {
    let events = [
        json!({"type":"user","message":{"content":"hello","role":"user"},
               "uuid":"u1","parentUuid":null,"sessionId":"test-sess",
               "timestamp":"2026-05-01T00:00:00Z"}),
        json!({"type":"assistant","message":{"model":"claude-opus-4-6","id":"msg1","role":"assistant",
               "content":[{"type":"text","text":"hi"}],"stop_reason":"end_turn",
               "usage":{"input_tokens":1,"output_tokens":1}},
               "uuid":"a1","parentUuid":"u1","sessionId":"test-sess",
               "timestamp":"2026-05-01T00:00:01Z"}),
    ];
    let content: String = events.iter().map(|e| format!("{}\n", e)).collect();
    fs::write(path, content).unwrap();
}

fn write_claude_transcript_cwd_on_later_line(path: &Path, cwd: &str) {
    let events = [
        json!({"type":"last-prompt","leafUuid":"abc","sessionId":"test-sess"}),
        json!({"type":"permission-mode","permissionMode":"default","sessionId":"test-sess"}),
        json!({"type":"user","message":{"content":"hello","role":"user"},
               "uuid":"u1","parentUuid":null,"cwd":cwd,"sessionId":"test-sess",
               "timestamp":"2026-05-01T00:00:00Z"}),
    ];
    let content: String = events.iter().map(|e| format!("{}\n", e)).collect();
    fs::write(path, content).unwrap();
}

fn setup_test_db(dir: &Path) -> StreamsDatabase {
    let db_path = dir.join("transcripts.db");
    StreamsDatabase::open(&db_path).unwrap()
}

fn make_stream_record(
    session_id: &str,
    tool: &str,
    stream_path: &Path,
    repo_work_dir: Option<&str>,
) -> StreamRecord {
    StreamRecord {
        session_id: session_id.to_string(),
        stream_kind: "transcript".to_string(),
        tool: tool.to_string(),
        stream_path: stream_path.display().to_string(),
        stream_format: StreamFormat::ClaudeJsonl,
        watermark_type: WatermarkType::ByteOffset,
        watermark_value: "0".to_string(),
        external_session_id: format!("ext-{}", session_id),
        external_parent_session_id: None,
        first_seen_at: chrono::Utc::now().timestamp(),
        last_processed_at: 0,
        last_known_size: 0,
        last_modified: None,
        processing_errors: 0,
        last_error: None,
        repo_work_dir: repo_work_dir.map(|s| s.to_string()),
    }
}

mod cwd_inference;
mod session_events;
mod url_resolution;
