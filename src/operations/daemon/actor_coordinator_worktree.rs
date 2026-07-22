#[allow(unused_imports)]
use super::*;
use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use crate::operations::git::repo_state::worktree_root_for_path;
use std::path::Path;

impl ActorDaemonCoordinator {
    pub(crate) fn worktree_state_key(worktree: &Path) -> String {
        let normalized = worktree_root_for_path(worktree).unwrap_or_else(|| worktree.to_path_buf());
        normalized
            .canonicalize()
            .unwrap_or(normalized)
            .to_string_lossy()
            .to_string()
    }

    pub(crate) fn set_pending_rebase_original_head_for_worktree(
        &self,
        worktree: &Path,
        original_head: String,
        onto: Option<String>,
    ) -> Result<(), GitAiError> {
        let mut map = self
            .pending_rebase_original_head_by_worktree
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending rebase original-head map",
            })?;
        map.insert(Self::worktree_state_key(worktree), (original_head, onto));
        Ok(())
    }

    pub(crate) fn clear_pending_rebase_original_head_for_worktree(
        &self,
        worktree: &Path,
    ) -> Result<(), GitAiError> {
        let mut map = self
            .pending_rebase_original_head_by_worktree
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending rebase original-head map",
            })?;
        map.remove(&Self::worktree_state_key(worktree));
        Ok(())
    }

    pub(crate) fn take_pending_rebase_original_head_for_worktree(
        &self,
        worktree: &Path,
    ) -> Result<Option<(String, Option<String>)>, GitAiError> {
        let mut map = self
            .pending_rebase_original_head_by_worktree
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending rebase original-head map",
            })?;
        Ok(map.remove(&Self::worktree_state_key(worktree)))
    }

    pub(crate) fn set_pending_cherry_pick_sources_for_worktree(
        &self,
        worktree: &Path,
        sources: Vec<String>,
    ) -> Result<(), GitAiError> {
        let mut map = self
            .pending_cherry_pick_sources_by_worktree
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending cherry-pick sources map",
            })?;
        let key = Self::worktree_state_key(worktree);
        if sources.is_empty() {
            map.remove(&key);
        } else {
            map.insert(key, sources);
        }
        Ok(())
    }

    pub(crate) fn clear_pending_cherry_pick_sources_for_worktree(
        &self,
        worktree: &Path,
    ) -> Result<(), GitAiError> {
        let mut map = self
            .pending_cherry_pick_sources_by_worktree
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending cherry-pick sources map",
            })?;
        map.remove(&Self::worktree_state_key(worktree));
        Ok(())
    }

    pub(crate) fn take_pending_cherry_pick_sources_for_worktree(
        &self,
        worktree: &Path,
    ) -> Result<Vec<String>, GitAiError> {
        let mut map = self
            .pending_cherry_pick_sources_by_worktree
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending cherry-pick sources map",
            })?;
        Ok(map
            .remove(&Self::worktree_state_key(worktree))
            .unwrap_or_default())
    }

    pub(crate) fn pending_cherry_pick_sources_for_worktree(
        &self,
        worktree: &Path,
    ) -> Result<Vec<String>, GitAiError> {
        let map = self
            .pending_cherry_pick_sources_by_worktree
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending cherry-pick sources map",
            })?;
        Ok(map
            .get(&Self::worktree_state_key(worktree))
            .cloned()
            .unwrap_or_default())
    }

    pub(crate) fn set_pending_cherry_pick_no_commit_for_worktree(
        &self,
        worktree: &Path,
        source_commits: Vec<String>,
        head: String,
    ) -> Result<(), GitAiError> {
        let mut map = self
            .pending_cherry_pick_no_commit_by_worktree
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending cherry-pick no-commit map",
            })?;
        let key = Self::worktree_state_key(worktree);
        if source_commits.is_empty() || head.is_empty() {
            map.remove(&key);
        } else {
            map.insert(
                key,
                PendingCherryPickNoCommit {
                    source_commits,
                    head,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn clear_pending_cherry_pick_no_commit_for_worktree(
        &self,
        worktree: &Path,
    ) -> Result<(), GitAiError> {
        let mut map = self
            .pending_cherry_pick_no_commit_by_worktree
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending cherry-pick no-commit map",
            })?;
        map.remove(&Self::worktree_state_key(worktree));
        Ok(())
    }

    pub(crate) fn take_pending_cherry_pick_no_commit_for_worktree(
        &self,
        worktree: &Path,
    ) -> Result<Option<PendingCherryPickNoCommit>, GitAiError> {
        let mut map = self
            .pending_cherry_pick_no_commit_by_worktree
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending cherry-pick no-commit map",
            })?;
        Ok(map.remove(&Self::worktree_state_key(worktree)))
    }

    pub(crate) fn set_pending_squash_merge_for_worktree(
        &self,
        worktree: &Path,
        source_head: String,
        onto: String,
    ) -> Result<(), GitAiError> {
        let mut map = self.pending_squash_merge_by_worktree.lock().map_err(|_| {
            PersistenceError::LockPoisoned {
                what: "pending squash merge map",
            }
        })?;
        map.insert(
            Self::worktree_state_key(worktree),
            PendingSquashMerge { source_head, onto },
        );
        Ok(())
    }

    pub(crate) fn take_pending_squash_merge_for_worktree(
        &self,
        worktree: &Path,
    ) -> Result<Option<PendingSquashMerge>, GitAiError> {
        let mut map = self.pending_squash_merge_by_worktree.lock().map_err(|_| {
            PersistenceError::LockPoisoned {
                what: "pending squash merge map",
            }
        })?;
        Ok(map.remove(&Self::worktree_state_key(worktree)))
    }

    pub(crate) fn resolve_heads_for_command(
        cmd: &crate::model::domain::NormalizedCommand,
    ) -> (String, String) {
        let old = cmd
            .ref_changes
            .iter()
            .find(|change| change.reference == "HEAD")
            .map(|change| change.old.clone())
            .or_else(|| {
                cmd.ref_changes
                    .iter()
                    .find(|change| change.reference.starts_with("refs/heads/"))
                    .map(|change| change.old.clone())
            })
            .or_else(|| {
                cmd.ref_changes
                    .iter()
                    .find(|change| is_non_auxiliary_ref(&change.reference))
                    .map(|change| change.old.clone())
            })
            .unwrap_or_default();
        let new = cmd
            .ref_changes
            .iter()
            .rfind(|change| change.reference == "HEAD")
            .map(|change| change.new.clone())
            .or_else(|| {
                cmd.ref_changes
                    .iter()
                    .rfind(|change| change.reference.starts_with("refs/heads/"))
                    .map(|change| change.new.clone())
            })
            .or_else(|| {
                cmd.ref_changes
                    .iter()
                    .rfind(|change| is_non_auxiliary_ref(&change.reference))
                    .map(|change| change.new.clone())
            })
            .unwrap_or_default();
        (old, new)
    }

    pub(crate) fn stash_pathspecs_from_command(
        cmd: &crate::model::domain::NormalizedCommand,
    ) -> Vec<String> {
        let parsed = parsed_invocation_for_normalized_command(cmd);
        if parsed.command.as_deref() != Some("stash") {
            return Vec::new();
        }

        let mut pathspecs = Vec::new();
        let mut found_separator = false;
        let mut skip_next = false;

        for (i, arg) in parsed.command_args.iter().enumerate() {
            if skip_next {
                skip_next = false;
                continue;
            }
            if arg == "--" {
                found_separator = true;
                continue;
            }
            if found_separator {
                pathspecs.push(arg.clone());
                continue;
            }
            if arg.starts_with('-') {
                if matches!(
                    arg.as_str(),
                    "-m" | "--message" | "--pathspec-from-file" | "--pathspec-file-nul"
                ) {
                    skip_next = true;
                }
                continue;
            }
            if i == 0 && matches!(arg.as_str(), "push" | "save" | "pop" | "apply") {
                continue;
            }
            if i == 1 && arg.starts_with("stash@") {
                continue;
            }
            pathspecs.push(arg.clone());
        }

        tracing::debug!("Extracted stash pathspecs: {:?}", pathspecs);
        pathspecs
    }
}
