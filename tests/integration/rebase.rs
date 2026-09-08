use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
#[cfg(not(target_os = "windows"))]
use crate::repos::write_executable_script;
use git_ai::model::authorship_log::PromptRecord;
use git_ai::model::authorship_log_serialization::AuthorshipLog;
use git_ai::model::working_log::AgentId;
use git_ai::operations::git::notes_api::write_note;
use std::collections::HashMap;

fn leading_dropped_commits_before_first_match(range_diff: &str) -> usize {
    let mut dropped = 0;
    for line in range_diff.lines() {
        let mut parts = line.split_whitespace();
        let (_ordinal, _old_sha, Some(status)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        match status {
            "<" => dropped += 1,
            "=" | "!" => break,
            _ => {}
        }
    }
    dropped
}

mod branch_selection;
mod conflict_recovery;
mod file_lifecycle;
mod historical_attribution;
mod interactive_edits;
mod line_attribution;
mod merge_and_stash;
mod note_metadata;
mod squash_authorship;
