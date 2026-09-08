use crate::operations::git::repo_storage::PersistedWorkingLog;
use crate::operations::git::repository::Repository;
use std::fs;
use std::path::PathBuf;

pub(super) fn stashes_dir(repo: &Repository) -> PathBuf {
    repo.storage.ai_dir.join("stashes")
}

pub(super) fn stashes_v2_dir(repo: &Repository) -> PathBuf {
    repo.storage.ai_dir.join("stashes_v2")
}

pub(super) fn cleanup_legacy_stashes_dir(repo: &Repository) {
    let legacy = stashes_dir(repo);
    if legacy.exists() {
        let _ = fs::remove_dir_all(legacy);
    }
}

pub(super) fn stash_entry_dir(repo: &Repository, stash_sha: &str) -> PathBuf {
    stashes_v2_dir(repo).join(stash_sha)
}

pub(super) fn stash_metadata_path(repo: &Repository, stash_sha: &str) -> PathBuf {
    stash_entry_dir(repo, stash_sha).join("metadata.json")
}

pub(super) fn filtered_stash_working_log_base(stash_sha: &str) -> String {
    format!("_stash_filter_{}", stash_sha)
}

pub(super) fn working_log_for_dir(
    repo: &Repository,
    dir: PathBuf,
    base_commit: &str,
) -> PersistedWorkingLog {
    let canonical_workdir =
        crate::operations::git::canonicalize::canonicalize_or_self(&repo.storage.repo_workdir);
    PersistedWorkingLog::new(
        dir,
        base_commit,
        repo.storage.repo_workdir.clone(),
        canonical_workdir,
        None,
    )
}
