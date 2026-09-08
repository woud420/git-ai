mod storage_files;
pub use storage_files::MAX_CHECKPOINTS_JSONL_BYTES;
use storage_files::create_directory_durably;
pub(crate) use storage_files::persist_file_version_to_blob_dir;

mod working_log;
pub use working_log::PersistedWorkingLog;

use crate::error::GitAiError;
#[cfg(test)]
use crate::model::attribution_tracker::LineAttribution;
use std::fs;
use std::path::{Path, PathBuf};

mod checkpoint_journal;

#[allow(unused_imports)]
pub(crate) use checkpoint_journal::LoadedJournal;

pub use crate::model::working_log::InitialAttributions;

#[derive(Debug, Clone)]
pub struct RepoStorage {
    pub ai_dir: PathBuf,
    pub repo_workdir: PathBuf,
    pub working_logs: PathBuf,
    pub logs: PathBuf,
}

impl RepoStorage {
    pub fn for_repo_path(repo_path: &Path, repo_workdir: &Path) -> Result<RepoStorage, GitAiError> {
        Self::for_ai_dir(&repo_path.join("ai"), repo_workdir)
    }

    pub fn for_isolated_worktree_storage(
        ai_dir: &Path,
        repo_workdir: &Path,
    ) -> Result<RepoStorage, GitAiError> {
        Self::for_ai_dir(ai_dir, repo_workdir)
    }

    fn for_ai_dir(ai_dir: &Path, repo_workdir: &Path) -> Result<RepoStorage, GitAiError> {
        let working_logs_dir = ai_dir.join("working_logs");
        let logs_dir = ai_dir.join("logs");

        let config = RepoStorage {
            ai_dir: ai_dir.to_path_buf(),
            repo_workdir: repo_workdir.to_path_buf(),
            working_logs: working_logs_dir,
            logs: logs_dir,
        };

        config.ensure_config_directory()?;
        Ok(config)
    }

    #[doc(hidden)]
    pub fn ensure_config_directory(&self) -> Result<(), GitAiError> {
        create_directory_durably(&self.ai_dir)?;
        create_directory_durably(&self.working_logs)?;
        create_directory_durably(&self.logs)?;

        Ok(())
    }

    /* Working Log Persistance */

    pub fn has_working_log(&self, sha: &str) -> bool {
        self.working_logs.join(sha).exists()
    }

    pub fn working_log_for_base_commit(
        &self,
        sha: &str,
    ) -> Result<PersistedWorkingLog, GitAiError> {
        let working_log_dir = self.working_logs.join(sha);
        create_directory_durably(&working_log_dir)?;
        let canonical_workdir =
            crate::operations::git::canonicalize::canonicalize_or_self(&self.repo_workdir);
        Ok(PersistedWorkingLog::new(
            working_log_dir,
            sha,
            self.repo_workdir.clone(),
            canonical_workdir,
            None,
        ))
    }

    pub fn delete_working_log_for_base_commit(&self, sha: &str) -> Result<(), GitAiError> {
        let working_log_dir = self.working_logs.join(sha);
        if working_log_dir.exists() {
            // Both debug and release: move to old-{sha} for retention
            let old_dir = self.working_logs.join(format!("old-{}", sha));
            // If old-{sha} already exists, remove it first
            if old_dir.exists() {
                fs::remove_dir_all(&old_dir)?;
            }
            fs::rename(&working_log_dir, &old_dir)?;
            crate::observability::wltrace::record(
                "working_log.gc.archive",
                &working_log_dir,
                || format!("to={:?}", old_dir),
            );

            // Write a timestamp marker so we know when it was archived
            let marker = old_dir.join(".archived_at");
            let now = crate::model::clock::now_secs();
            // Best-effort; don't fail the commit if we can't write the marker
            let _ = fs::write(&marker, now.to_string());

            tracing::debug!("Moved checkpoint directory from {} to old-{}", sha, sha);

            // In production builds, prune old working logs that have expired.
            // Debug builds never prune so developers can inspect old state.
            if !cfg!(debug_assertions) {
                self.prune_expired_old_working_logs();
            }
        }
        Ok(())
    }

    /// Number of seconds to retain archived working logs in production builds (7 days).
    const OLD_WORKING_LOG_RETENTION_SECS: u64 = 7 * 24 * 60 * 60;

    /// Remove archived (`old-*`) working log directories whose `.archived_at`
    /// timestamp is older than `OLD_WORKING_LOG_RETENTION_SECS`.
    /// Errors are intentionally swallowed so pruning never breaks the commit flow.
    #[doc(hidden)]
    pub fn prune_expired_old_working_logs(&self) {
        let now_secs = crate::model::clock::now_secs();

        let entries = match fs::read_dir(&self.working_logs) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.starts_with("old-") {
                continue;
            }

            let dir_path = entry.path();
            if !dir_path.is_dir() {
                continue;
            }

            let marker = dir_path.join(".archived_at");
            let archived_at = match fs::read_to_string(&marker) {
                Ok(contents) => contents.trim().parse::<u64>().unwrap_or(0),
                // No marker means this was created before the retention feature;
                // treat it as immediately expired so it gets cleaned up.
                Err(_) => 0,
            };

            if now_secs.saturating_sub(archived_at) >= Self::OLD_WORKING_LOG_RETENTION_SECS {
                tracing::debug!("Pruning expired old working log: {}", name_str);
                if fs::remove_dir_all(&dir_path).is_ok() {
                    crate::observability::wltrace::record(
                        "working_log.gc.prune",
                        &dir_path,
                        String::new,
                    );
                }
            }
        }
    }

    /// Move a working log directory from one commit SHA to another.
    /// If the destination already has checkpoints, preserve the old-base entries first and
    /// append the destination entries after them.
    pub fn rename_working_log(&self, old_sha: &str, new_sha: &str) -> Result<(), GitAiError> {
        let old_dir = self.working_logs.join(old_sha);
        let new_dir = self.working_logs.join(new_sha);
        if !old_dir.exists() {
            return Ok(());
        }
        if !new_dir.exists() {
            fs::rename(&old_dir, &new_dir)?;
            crate::observability::wltrace::record("working_log.rename", &old_dir, || {
                format!("to={:?}", new_dir)
            });
            tracing::debug!("Renamed working log from {} to {}", old_sha, new_sha);
        } else {
            self.merge_working_log_dirs(old_sha, new_sha, &old_dir, &new_dir)?;
            fs::remove_dir_all(&old_dir)?;
            crate::observability::wltrace::record("working_log.merge", &old_dir, || {
                format!("to={:?}", new_dir)
            });
            tracing::debug!("Merged working log from {} into {}", old_sha, new_sha);
        }
        Ok(())
    }

    fn merge_working_log_dirs(
        &self,
        old_sha: &str,
        new_sha: &str,
        old_dir: &Path,
        new_dir: &Path,
    ) -> Result<(), GitAiError> {
        copy_dir_contents(&old_dir.join("blobs"), &new_dir.join("blobs"))?;

        let canonical =
            crate::operations::git::canonicalize::canonicalize_or_self(&self.repo_workdir);
        let old_log = PersistedWorkingLog::new(
            old_dir.to_path_buf(),
            old_sha,
            self.repo_workdir.clone(),
            canonical.clone(),
            None,
        );
        let new_log = PersistedWorkingLog::new(
            new_dir.to_path_buf(),
            new_sha,
            self.repo_workdir.clone(),
            canonical,
            None,
        );

        // Preserve OLD-base entries first (per rename_working_log's contract):
        // start from the old INITIAL and only insert a new-base entry when its
        // key is absent, so old wins on any shared key. HashMap::extend would do
        // the opposite (new clobbers old). The checkpoints Vec below is already
        // old-then-new, so it needs no such guard.
        let mut merged_initial = old_log.read_initial_attributions();
        let new_initial = new_log.read_initial_attributions();
        for (k, v) in new_initial.files {
            merged_initial.files.entry(k).or_insert(v);
        }
        for (k, v) in new_initial.prompts {
            merged_initial.prompts.entry(k).or_insert(v);
        }
        for (k, v) in new_initial.file_blobs {
            merged_initial.file_blobs.entry(k).or_insert(v);
        }
        for (k, v) in new_initial.humans {
            merged_initial.humans.entry(k).or_insert(v);
        }
        for (k, v) in new_initial.sessions {
            merged_initial.sessions.entry(k).or_insert(v);
        }
        new_log.write_initial(merged_initial)?;

        let mut checkpoints = old_log.read_all_checkpoints()?;
        checkpoints.extend(new_log.read_all_checkpoints()?);
        new_log.write_all_checkpoints(&checkpoints)?;
        Ok(())
    }
}

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), GitAiError> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)?.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[path = "repo_storage_tests.rs"]
#[cfg(test)]
mod tests;
