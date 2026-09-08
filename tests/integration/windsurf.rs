use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::read_jsonl_fixture;
use git_ai::error::GitAiError;
use git_ai::model::stream_watermark::ByteOffsetWatermark;
use git_ai::operations::commands::checkpoint_agent::presets::{ParsedHookEvent, resolve_preset};
use git_ai::operations::streams::agents::WindsurfAgent;
use serde_json::json;
use std::fs;
use std::thread;
use std::time::Duration;

fn parse_windsurf(hook_input: &str) -> Result<Vec<ParsedHookEvent>, GitAiError> {
    resolve_preset("windsurf")?.parse(hook_input, "t_test")
}

mod bash_commands;
mod pending_edits;
mod preset_and_transcript;
