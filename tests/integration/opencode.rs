use crate::test_utils::fixture_path;
use git_ai::error::GitAiError;
use git_ai::operations::commands::checkpoint_agent::presets::{ParsedHookEvent, resolve_preset};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn parse_and_enrich_opencode(hook_input: &str) -> Result<Vec<ParsedHookEvent>, GitAiError> {
    let preset = resolve_preset("opencode")?;
    let mut events = preset.parse(hook_input, "t_test")?;
    preset.enrich_authorized_events(hook_input, &mut events)?;
    Ok(events)
}

fn opencode_sqlite_fixture_path() -> std::path::PathBuf {
    fixture_path("opencode-sqlite")
}

mod checkpoint_cycles;
mod tool_identity;
