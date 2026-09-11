use crate::model::stream_types::StreamError;
use crate::operations::streams::codex_model::extract_model_from_codex_jsonl;
use crate::operations::streams::copilot_model::extract_model_from_copilot_session_json;
use crate::operations::streams::jsonl_scan::scan_jsonl;
use crate::operations::streams::sweep::StreamFormat;
use std::path::{Path, PathBuf};

pub fn extract_model(
    path: &Path,
    format: StreamFormat,
    session_id: Option<&str>,
) -> Result<Option<String>, StreamError> {
    match format {
        StreamFormat::ClaudeJsonl
        | StreamFormat::CopilotEventStreamJsonl
        | StreamFormat::GeminiJsonl => scan_jsonl(path, extract_model_from_jsonl_line),
        StreamFormat::CodexJsonl => extract_model_from_codex_jsonl(path),
        StreamFormat::CopilotSessionJson => extract_model_from_copilot_session_json(path),
        StreamFormat::AmpThreadJson => extract_model_from_amp_thread_json(path),
        StreamFormat::OpenCodeSqlite => extract_model_from_opencode_sqlite(path, session_id),
        StreamFormat::CopilotOtelSqlite => extract_model_from_copilot_otel_sqlite(path, session_id),
        // Droid uses extract_model_from_droid_settings() with the settings path instead
        _ => Ok(None),
    }
}

pub fn extract_model_from_droid_settings(
    settings_path: &Path,
) -> Result<Option<String>, StreamError> {
    let content = match std::fs::read_to_string(settings_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok(None),
        Err(_) => return Ok(None),
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    Ok(json.get("model").and_then(|v| v.as_str()).map(String::from))
}

fn extract_model_from_jsonl_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(trimmed).ok()?;

    if json.get("type").and_then(|v| v.as_str()) == Some("session.model_change")
        && let Some(model) = json
            .get("data")
            .and_then(|d| d.get("newModel"))
            .and_then(|v| v.as_str())
    {
        return Some(model.to_string());
    }

    let candidate = json
        .get("message")
        .and_then(|m| m.get("model"))
        .and_then(|v| v.as_str())
        .or_else(|| json.get("model").and_then(|v| v.as_str()));

    candidate.and_then(normalize_model)
}

pub(crate) fn normalize_model(model: &str) -> Option<String> {
    let model = model.trim();
    if model.is_empty() || model == "<synthetic>" {
        return None;
    }
    Some(model.to_string())
}

/// Extracts the model from VS Code Copilot's `models.json` debug log.
/// Given a transcript path like `.../transcripts/{session_id}.jsonl`,
/// derives `.../debug-logs/{session_id}/models.json` and reads the default model.
pub fn extract_model_from_copilot_models_json(
    stream_path: &Path,
) -> Result<Option<String>, StreamError> {
    let session_id = stream_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if session_id.is_empty() {
        return Ok(None);
    }

    // transcript: .../transcripts/{session_id}.jsonl
    // models:     .../debug-logs/{session_id}/models.json
    let transcripts_dir = match stream_path.parent() {
        Some(p) => p,
        None => return Ok(None),
    };
    let copilot_chat_dir = match transcripts_dir.parent() {
        Some(p) => p,
        None => return Ok(None),
    };
    let models_path = copilot_chat_dir
        .join("debug-logs")
        .join(session_id)
        .join("models.json");

    let content = match std::fs::read_to_string(&models_path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let models: Vec<serde_json::Value> = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let model = models.iter().find_map(|m| {
        if m.get("is_chat_default").and_then(|v| v.as_bool()) == Some(true) {
            m.get("id").and_then(|v| v.as_str()).map(String::from)
        } else {
            None
        }
    });

    Ok(model)
}

pub fn extract_model_from_copilot_vscode_transcript(
    stream_path: &Path,
    format: StreamFormat,
    chat_session_id: &str,
) -> Result<Option<String>, StreamError> {
    if let Some(model) = extract_model(stream_path, format, None)? {
        return Ok(Some(model));
    }

    if let Some(model) =
        extract_model_from_copilot_otel_for_transcript(stream_path, chat_session_id)?
    {
        return Ok(Some(model));
    }

    extract_model_from_copilot_models_json(stream_path)
}

pub fn extract_model_from_copilot_otel_for_transcript(
    stream_path: &Path,
    chat_session_id: &str,
) -> Result<Option<String>, StreamError> {
    let Some(db_path) = resolve_copilot_otel_db_path(stream_path) else {
        return Ok(None);
    };
    extract_model_from_copilot_otel_sqlite(&db_path, Some(chat_session_id))
}

fn resolve_copilot_otel_db_path(stream_path: &Path) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("GIT_AI_COPILOT_OTEL_DB_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    // transcript: .../User/workspaceStorage/{hash}/GitHub.copilot-chat/transcripts/{id}.jsonl
    // OTEL DB:    .../User/globalStorage/github.copilot-chat/agent-traces.db
    let workspace_storage_root = stream_path.parent()?.parent()?.parent()?.parent()?;
    let user_dir = workspace_storage_root.parent()?;
    let otel_db = user_dir
        .join("globalStorage")
        .join("github.copilot-chat")
        .join("agent-traces.db");

    otel_db.exists().then_some(otel_db)
}

fn extract_model_from_copilot_otel_sqlite(
    path: &Path,
    chat_session_id: Option<&str>,
) -> Result<Option<String>, StreamError> {
    let Some(chat_session_id) = chat_session_id.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    let conn = match crate::operations::streams::agents::opencode::open_sqlite_readonly(path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let newest_request_model: Option<String> = conn
        .query_row(
            "SELECT request_model FROM spans \
             WHERE chat_session_id = ?1 AND request_model IS NOT NULL AND request_model != '' \
             ORDER BY end_time_ms DESC, span_id DESC LIMIT 1",
            rusqlite::params![chat_session_id],
            |row| row.get(0),
        )
        .ok();

    if newest_request_model.is_some() {
        return Ok(newest_request_model);
    }

    let newest_response_model: Option<String> = conn
        .query_row(
            "SELECT response_model FROM spans \
             WHERE chat_session_id = ?1 AND response_model IS NOT NULL AND response_model != '' \
             ORDER BY end_time_ms DESC, span_id DESC LIMIT 1",
            rusqlite::params![chat_session_id],
            |row| row.get(0),
        )
        .ok();

    Ok(newest_response_model)
}

fn extract_model_from_amp_thread_json(path: &Path) -> Result<Option<String>, StreamError> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let model = json
        .get("messages")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|msg| {
                msg.get("usage")
                    .and_then(|u| u.get("model"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
        });

    Ok(model)
}

fn extract_model_from_opencode_sqlite(
    path: &Path,
    session_id: Option<&str>,
) -> Result<Option<String>, StreamError> {
    let conn = match crate::operations::streams::agents::opencode::open_sqlite_readonly(path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    // OpenCode stores model info in two places depending on message role:
    //   User messages:     data.model.modelID  (nested object)
    //   Assistant messages: data.modelID        (top-level string)
    let (query, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match session_id {
        Some(sid) => (
            "SELECT data FROM message WHERE session_id = ? AND (data LIKE '%\"modelID\"%' OR data LIKE '%\"model\"%') LIMIT 1",
            vec![Box::new(sid.to_string())],
        ),
        None => (
            "SELECT data FROM message WHERE (data LIKE '%\"modelID\"%' OR data LIKE '%\"model\"%') LIMIT 1",
            vec![],
        ),
    };

    let result: Option<String> = conn
        .query_row(query, rusqlite::params_from_iter(params.iter()), |row| {
            row.get::<_, String>(0)
        })
        .ok()
        .and_then(|data| {
            let json: serde_json::Value = serde_json::from_str(&data).ok()?;
            // Try user message format: data.model.modelID
            if let Some(model) = json
                .get("model")
                .and_then(|m| m.get("modelID"))
                .and_then(|v| v.as_str())
            {
                return Some(model.to_string());
            }
            // Try assistant message format: data.modelID
            json.get("modelID")
                .and_then(|v| v.as_str())
                .map(String::from)
        });

    Ok(result)
}

#[path = "model_extraction_tests.rs"]
#[cfg(test)]
mod tests;
