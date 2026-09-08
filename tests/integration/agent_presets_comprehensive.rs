use git_ai::error::GitAiError;
use git_ai::model::stream_watermark::ByteOffsetWatermark;
use git_ai::operations::commands::checkpoint_agent::presets::{ParsedHookEvent, resolve_preset};
use git_ai::operations::streams::agents::{ClaudeAgent, GeminiAgent};
use serde_json::json;
use std::fs;

#[track_caller]
fn preset_error_message(
    result: Result<Vec<ParsedHookEvent>, GitAiError>,
    unexpected: &'static str,
) -> String {
    assert!(result.is_err());
    match result {
        Err(GitAiError::PresetError(msg)) => msg,
        // Preserve the static-string panic payload used by the individual tests.
        _ => std::panic::panic_any(unexpected),
    }
}

mod claude;
mod continue_and_codex_presets;
mod editor_agents;
mod gemini_presets;
