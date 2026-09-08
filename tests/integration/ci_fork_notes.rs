// repos module is declared once in tests/integration/main.rs
use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::operations::ci::ci_context::{CiContext, CiEvent, CiRunResult};
use git_ai::operations::git::notes_api::read_authorship_v3;
use git_ai::operations::git::notes_api::{read_note, write_note};
use git_ai::operations::git::repository as GitAiRepository;
use std::fs;

/// Helper: set up "origin" as a self-referencing remote so fetch_authorship_notes("origin")
/// doesn't fail. In real CI the repo is cloned from origin, so it always exists.
fn add_self_origin(repo: &TestRepo) {
    let path = repo.path().to_str().unwrap();
    repo.git_og(&["remote", "add", "origin", path]).ok(); // ok() in case it already exists
}

mod merge_commit;
mod notes_filtering;
mod squash_merge;
