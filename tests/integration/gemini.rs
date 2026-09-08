use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::{fixture_path, read_jsonl_fixture};
use git_ai::error::GitAiError;
use git_ai::model::stream_watermark::ByteOffsetWatermark;
use git_ai::operations::commands::checkpoint_agent::presets::{ParsedHookEvent, resolve_preset};
use git_ai::operations::streams::agents::GeminiAgent;
use serde_json::json;
use std::fs;

fn parse_gemini(hook_input: &str) -> Result<Vec<ParsedHookEvent>, GitAiError> {
    let preset = resolve_preset("gemini")?;
    let mut events = preset.parse(hook_input, "t_test")?;
    preset.enrich_authorized_events(hook_input, &mut events)?;
    Ok(events)
}

mod preset_parsing;
mod tool_cycles;
