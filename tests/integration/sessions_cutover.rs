// Critical regression tests for old-format/new-format coexistence during cutover scenarios
//
// These tests verify that git-ai correctly handles:
// 1. Old-format authorship notes (bare 16-char hex hashes, prompts-only metadata)
// 2. New-format authorship notes (s_::t_ hashes, sessions metadata)
// 3. Mixed scenarios where both formats coexist in the same note or across operations
//
// Format detection: checkpoint.trace_id.is_some() determines which format is used.

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::authorship_log_serialization::AuthorshipLog;
use git_ai::operations::git::notes_api::write_note;
use git_ai::operations::git::repo_storage::PersistedWorkingLog;
use serde_json::Value;
use std::fs;

fn rewrite_checkpoint_journal_as_legacy(working_log: &PersistedWorkingLog) {
    let content = working_log
        .read_all_checkpoints()
        .unwrap()
        .into_iter()
        .map(|checkpoint| serde_json::to_string(&checkpoint).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(working_log.checkpoints_file(), content).unwrap();
}

mod amend_compatibility;
mod amend_line_attribution;
mod diff_formats;
mod legacy_notes;
mod prompt_lookup;
mod rewrite_compatibility;
mod working_log_formats;
