use crate::repos::test_repo::TestRepo;
use git_ai::error::GitAiError;
use git_ai::model::attribution_tracker::LineAttribution;
use git_ai::model::working_log::{AgentId, CHECKPOINT_API_VERSION, Checkpoint, CheckpointKind};
use git_ai::operations::git::repo_storage::{InitialAttributions, RepoStorage};
use std::collections::HashMap;
use std::fs;
use std::time::SystemTime;

/// Helper: create a RepoStorage for a TestRepo.
fn storage_for(repo: &TestRepo) -> RepoStorage {
    let git_dir = repo.path().join(".git");
    let workdir = repo.path();
    RepoStorage::for_repo_path(&git_dir, workdir.as_path()).unwrap()
}

mod checkpoint_journals;
mod lifecycle;
mod snapshot_blobs;
