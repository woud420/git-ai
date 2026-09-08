use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::authorship_log::PromptRecord;
use git_ai::model::working_log::AgentId;
use git_ai::operations::git::notes_api::write_note;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn git_common_dir(repo: &TestRepo) -> PathBuf {
    let raw = repo
        .git_og(&["rev-parse", "--git-common-dir"])
        .expect("rev-parse --git-common-dir should succeed");
    let common_dir = PathBuf::from(raw.trim());
    if common_dir.is_absolute() {
        common_dir
    } else {
        repo.path().join(common_dir)
    }
}

mod conflict_recovery;
mod line_attribution;
mod note_metadata;
mod remote_sources;
