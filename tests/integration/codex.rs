use crate::repos::test_file::ExpectedLineExt;
use crate::test_utils::{CodexHookInput, checkpoint_codex, fixture_path, read_jsonl_fixture};
use git_ai::error::GitAiError;
use git_ai::model::stream_watermark::ByteOffsetWatermark;
use git_ai::operations::commands::checkpoint_agent::presets::{ParsedHookEvent, resolve_preset};
use git_ai::operations::streams::agents::CodexAgent;
use serde_json::json;
use std::fs;

fn parse_codex(hook_input: &str) -> Result<Vec<ParsedHookEvent>, GitAiError> {
    resolve_preset("codex")?.parse(hook_input, "t_test")
}

mod apply_patch;
mod bash_hook_parsing;
mod checkpoint_ownership;
mod inflight_commit;
mod rollout_identity;
mod tool_cycles;
