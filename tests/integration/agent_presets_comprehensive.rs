use git_ai::error::GitAiError;
use git_ai::model::stream_watermark::ByteOffsetWatermark;
use git_ai::operations::commands::checkpoint_agent::presets::{ParsedHookEvent, resolve_preset};
use git_ai::operations::streams::agents::{ClaudeAgent, GeminiAgent};
use serde_json::json;
use std::fs;

mod claude;
mod continue_and_codex_presets;
mod editor_agents;
mod gemini_presets;
