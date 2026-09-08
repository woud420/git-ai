mod sync;
#[cfg(test)]
use sync::sync_fetch_remote_supports_lazy_blobs;
use sync::{
    commit_is_ancestor, commit_ranges_have_same_patch_ids, commits_in_range_oldest_first,
    count_commits_with_authorship_notes, ensure_commit_available_for_sync,
};

use crate::error::GitAiError;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::model::domain::RewriteEvent;
use crate::operations::authorship::rewrite::handle_rewrite_event;
use crate::operations::git::notes_api::{read_authorship_v3, read_note};
use crate::operations::git::repository::{CommitRange, Repository};
use crate::operations::git::sync_authorship::fetch_authorship_notes;
use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
pub enum CiEvent {
    Merge {
        merge_commit_sha: String,
        head_ref: String,
        head_sha: String,
        base_ref: String,
        base_sha: String,
        /// Clone URL of the fork repository, if this PR came from a fork.
        /// When set, notes will be fetched from the fork before processing.
        fork_clone_url: Option<String>,
    },
    Sync {
        previous_head_sha: String,
        head_sha: String,
        base_ref: String,
        base_sha: String,
        previous_base_sha: Option<String>,
        previous_head_fetch_remote: Option<String>,
    },
}

/// Result of running CiContext
#[derive(Debug)]
pub enum CiRunResult {
    /// Authorship was successfully rewritten for a squash/rebase merge
    AuthorshipRewritten {
        #[allow(dead_code)]
        authorship_log: AuthorshipLog,
    },
    /// Authorship was successfully rewritten for one or more rebased commits
    SyncAuthorshipRewritten {
        #[allow(dead_code)]
        commit_count: usize,
    },
    /// Skipped: merge commit has multiple parents (simple merge - authorship already present)
    SkippedSimpleMerge,
    /// Skipped: merge commit equals head (fast-forward - no rewrite needed)
    SkippedFastForward,
    /// Skipped: the PR synchronize event was not a rebase-like rewrite
    SkippedNonRebaseSync,
    /// Skipped: one or more current PR commits already have authorship notes
    SkippedExistingSyncNotes,
    /// Authorship already exists for this commit
    AlreadyExists {
        #[allow(dead_code)]
        authorship_log: AuthorshipLog,
    },
    /// Fork notes were fetched and preserved for a merge commit from a fork
    ForkNotesPreserved,
    /// No AI authorship to track (pre-git-ai commits or human-only code)
    NoAuthorshipAvailable,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CiRunOptions {
    pub skip_fetch_notes: bool,
    pub skip_fetch_base: bool,
    pub skip_fetch_fork_notes: bool,
    pub skip_fetch_sync_refs: bool,
    pub skip_push: bool,
}

#[derive(Debug)]
pub struct CiContext {
    pub repo: Repository,
    pub event: CiEvent,
    pub temp_dir: PathBuf,
}

impl CiContext {
    /// Create a CiContext with an existing repository (no automatic cleanup)
    #[allow(dead_code)]
    pub fn with_repository(repo: Repository, event: CiEvent) -> Self {
        CiContext {
            repo,
            event,
            temp_dir: PathBuf::new(), // Empty path indicates no cleanup needed
        }
    }

    pub fn run(&self) -> Result<CiRunResult, GitAiError> {
        self.run_with_options(CiRunOptions::default())
    }

    pub fn run_with_options(&self, options: CiRunOptions) -> Result<CiRunResult, GitAiError> {
        match &self.event {
            CiEvent::Merge {
                merge_commit_sha,
                head_ref: _,
                head_sha,
                base_ref,
                base_sha,
                fork_clone_url,
            } => {
                println!("Working repository is in {}", self.repo.path().display());

                if options.skip_fetch_notes {
                    println!("Skipping authorship history fetch");
                } else {
                    println!("Fetching authorship history");
                    // Ensure we have the full authorship history before checking for existing notes
                    fetch_authorship_notes(&self.repo, "origin")?;
                    println!("Fetched authorship history");
                }

                // Check if authorship already exists for this commit
                match read_authorship_v3(&self.repo, merge_commit_sha) {
                    Ok(existing_log) => {
                        println!("{} already has authorship", merge_commit_sha);
                        return Ok(CiRunResult::AlreadyExists {
                            authorship_log: existing_log,
                        });
                    }
                    Err(e) => {
                        if read_note(&self.repo, merge_commit_sha).is_some() {
                            return Err(e);
                        }
                    }
                }

                // Only handle squash or rebase-like merges.
                // Skip simple merge commits (2+ parents) and fast-forward merges (merge commit == head).
                let merge_commit = self.repo.find_commit(merge_commit_sha.clone())?;
                let parent_count = merge_commit.parents().count();
                if parent_count > 1 {
                    // For fork PRs with merge commits, the merged commits keep
                    // their fork SHAs. Import only notes for those PR commits,
                    // then push the scoped local authorship ref.
                    if fork_clone_url.is_some() {
                        let (_source_base, original_commits) = self
                            .original_pr_commits_for_merge_commit(
                                &merge_commit,
                                head_sha,
                                base_ref,
                                base_sha,
                            );
                        let fork_notes_imported = self.import_fork_notes_for_commits(
                            fork_clone_url,
                            &original_commits,
                            options,
                        )?;
                        if !self.has_notes_for_any_commit(&original_commits)? {
                            println!(
                                "No local authorship notes available for fork PR commits; skipping fork note push"
                            );
                            return Ok(CiRunResult::SkippedSimpleMerge);
                        }

                        println!(
                            "{} has {} parents (merge commit from fork) - preserving fork notes",
                            merge_commit_sha, parent_count
                        );
                        if fork_notes_imported > 0 {
                            println!(
                                "Imported {} fork authorship notes for PR commits",
                                fork_notes_imported
                            );
                        } else {
                            println!(
                                "Using existing local authorship notes (no additional fork notes fetched)"
                            );
                        }
                        if options.skip_push {
                            println!("Skipping authorship push (--skip-push). Done.");
                        } else {
                            println!("Pushing authorship...");
                            self.repo.push_authorship("origin")?;
                            println!("Pushed authorship. Done.");
                        }
                        return Ok(CiRunResult::ForkNotesPreserved);
                    }
                    println!(
                        "{} has {} parents (simple merge)",
                        merge_commit_sha, parent_count
                    );
                    return Ok(CiRunResult::SkippedSimpleMerge);
                }

                if merge_commit_sha == head_sha {
                    if fork_clone_url.is_some() {
                        let (_source_base, original_commits) =
                            self.original_pr_commits(head_sha, base_ref, base_sha);
                        let fork_notes_imported = self.import_fork_notes_for_commits(
                            fork_clone_url,
                            &original_commits,
                            options,
                        )?;
                        if self.has_notes_for_any_commit(&original_commits)? {
                            println!(
                                "{} equals head {} (fast-forward from fork) - preserving fork notes",
                                merge_commit_sha, head_sha
                            );
                            println!(
                                "Imported {} fork authorship notes for PR commits",
                                fork_notes_imported
                            );
                            if options.skip_push {
                                println!("Skipping authorship push (--skip-push). Done.");
                            } else {
                                println!("Pushing authorship...");
                                self.repo.push_authorship("origin")?;
                                println!("Pushed authorship. Done.");
                            }
                            return Ok(CiRunResult::ForkNotesPreserved);
                        }
                    }
                    println!(
                        "{} equals head {} (fast-forward)",
                        merge_commit_sha, head_sha
                    );
                    return Ok(CiRunResult::SkippedFastForward);
                }
                println!(
                    "Rewriting authorship for {} -> {} (squash or rebase-like merge)",
                    head_sha, merge_commit_sha
                );
                if options.skip_fetch_base {
                    println!("Skipping base branch fetch for {}", base_ref);
                    self.repo.revparse_single(base_ref).map_err(|e| {
                        GitAiError::Generic(format!(
                            "Failed to resolve base ref '{}' locally while --skip-fetch-base is set: {}",
                            base_ref, e
                        ))
                    })?;
                } else {
                    println!("Fetching base branch {}", base_ref);
                    // Ensure we have all the required commits from the base branch
                    self.repo.fetch_branch(base_ref, "origin").map_err(|e| {
                        GitAiError::Generic(format!(
                            "Failed to fetch base branch '{}': {}",
                            base_ref, e
                        ))
                    })?;
                    println!("Fetched base branch.");
                }

                // Detect squash vs rebase merge by counting commits:
                //   squash: N original commits → 1 merge commit
                //   rebase: N original commits → N rebased commits
                let (original_commits_base, original_commits) =
                    self.original_pr_commits(head_sha, base_ref, base_sha);

                println!(
                    "Original commits in PR: {} (from {:?})",
                    original_commits.len(),
                    original_commits_base
                );

                self.import_fork_notes_for_commits(fork_clone_url, &original_commits, options)?;

                // For multi-commit PRs, decide whether the merge is a rebase
                // (N original → N new commits) or a squash (N → 1) by walking
                // back from merge_commit_sha.
                let is_rebase_merge = if original_commits.len() > 1 {
                    let mut new_commits =
                        self.get_rebased_commits(merge_commit_sha, original_commits.len());

                    // #1473: on a linear base branch the first-parent walk above can
                    // return pre-existing base commits rather than rebased PR commits,
                    // so a squash merge's count matches a rebase's and gets
                    // misclassified (PR notes then land on unrelated commits). Restrict
                    // to commits the merge actually introduced
                    // (`base_sha..merge_commit_sha`; see gitrevisions(7)) — a squash
                    // yields exactly one, so it can't look like a rebase. An empty
                    // `base_sha` (transient API failure) safely skips the filter and
                    // falls back to the pre-#1473 behavior.
                    if !base_sha.is_empty() {
                        let introduced: std::collections::HashSet<String> =
                            CommitRange::new_infer_refname(
                                &self.repo,
                                base_sha.clone(),
                                merge_commit_sha.to_string(),
                                None,
                            )
                            .map(|r| r.all_commits())
                            .unwrap_or_default()
                            .into_iter()
                            .collect();
                        if !introduced.is_empty() {
                            new_commits.retain(|sha| introduced.contains(sha));
                        }
                    }

                    new_commits.len() == original_commits.len()
                } else {
                    false
                };

                if is_rebase_merge {
                    println!(
                        "Detected rebase merge: {} original commits → {} new commits",
                        original_commits.len(),
                        original_commits.len()
                    );
                    // Rebase merge — shift each original commit's note onto its
                    // rebased counterpart via the range-diff/hunk-shift path.
                    handle_rewrite_event(
                        &self.repo,
                        RewriteEvent::NonFastForward {
                            old_tip: head_sha.to_string(),
                            new_tip: merge_commit_sha.to_string(),
                            onto: if base_sha.is_empty() {
                                None
                            } else {
                                Some(base_sha.to_string())
                            },
                        },
                    )?;
                } else {
                    println!(
                        "Detected squash merge: {} original commit(s) → 1 merge commit",
                        original_commits.len()
                    );
                    // Squash merge — reconstruct the single merge commit's
                    // authorship by unioning every source commit's note, using the
                    // exact same handler the local daemon uses for `merge --squash`.
                    let onto = if base_sha.is_empty() {
                        // No base SHA: fall back to the merge commit's first parent
                        // so the squash handler can still enumerate source commits.
                        self.repo
                            .find_commit(merge_commit_sha.to_string())
                            .ok()
                            .and_then(|c| c.parent(0).ok())
                            .map(|p| p.id())
                            .unwrap_or_else(|| base_ref.to_string())
                    } else {
                        base_sha.to_string()
                    };
                    handle_rewrite_event(
                        &self.repo,
                        RewriteEvent::SquashMerge {
                            source_head: head_sha.to_string(),
                            squash_commit: merge_commit_sha.to_string(),
                            onto,
                        },
                    )?;
                }
                println!("Rewrote authorship.");

                // Check if authorship was created for THIS specific commit
                match read_authorship_v3(&self.repo, merge_commit_sha) {
                    Ok(authorship_log) => {
                        // A note may be reconstructed with only human attestations
                        // (e.g. a PR whose contributor never used git-ai, so there
                        // are no AI sessions/prompts to carry forward). There is no
                        // AI authorship to track in that case.
                        let has_ai_authorship = !authorship_log.metadata.sessions.is_empty()
                            || !authorship_log.metadata.prompts.is_empty();
                        if !has_ai_authorship {
                            println!(
                                "No AI authorship to track for this commit (no AI-touched files in PR)"
                            );
                            return Ok(CiRunResult::NoAuthorshipAvailable);
                        }
                        if options.skip_push {
                            println!("Skipping authorship push (--skip-push). Done.");
                        } else {
                            println!("Pushing authorship...");
                            self.repo.push_authorship("origin")?;
                            println!("Pushed authorship. Done.");
                        }
                        Ok(CiRunResult::AuthorshipRewritten { authorship_log })
                    }
                    Err(e) => {
                        if read_note(&self.repo, merge_commit_sha).is_some() {
                            return Err(e);
                        }
                        println!(
                            "No AI authorship to track for this commit (no AI-touched files in PR)"
                        );
                        Ok(CiRunResult::NoAuthorshipAvailable)
                    }
                }
            }
            CiEvent::Sync {
                previous_head_sha,
                head_sha,
                base_ref,
                base_sha,
                previous_base_sha,
                previous_head_fetch_remote,
            } => {
                println!("Working repository is in {}", self.repo.path().display());

                if options.skip_fetch_notes {
                    println!("Skipping authorship history fetch");
                } else {
                    println!("Fetching authorship history");
                    fetch_authorship_notes(&self.repo, "origin")?;
                    println!("Fetched authorship history");
                }

                if previous_head_sha == head_sha {
                    println!(
                        "{} equals previous head {} (no head rewrite)",
                        head_sha, previous_head_sha
                    );
                    return Ok(CiRunResult::SkippedFastForward);
                }
                ensure_commit_available_for_sync(
                    &self.repo,
                    previous_head_sha,
                    previous_head_fetch_remote.as_deref().unwrap_or("origin"),
                    "refs/git-ai/ci-sync/previous-head",
                    options.skip_fetch_sync_refs,
                )?;
                ensure_commit_available_for_sync(
                    &self.repo,
                    head_sha,
                    "origin",
                    "refs/git-ai/ci-sync/head",
                    options.skip_fetch_sync_refs,
                )?;

                if commit_is_ancestor(&self.repo, previous_head_sha, head_sha)? {
                    println!(
                        "{} is an ancestor of {} (fast-forward PR update)",
                        previous_head_sha, head_sha
                    );
                    return Ok(CiRunResult::SkippedFastForward);
                }

                let base_target =
                    if !base_sha.is_empty() && self.repo.revparse_single(base_sha).is_ok() {
                        base_sha.as_str()
                    } else {
                        base_ref.as_str()
                    };
                let resolved_previous_base_sha = match previous_base_sha {
                    Some(previous_base_sha) if !previous_base_sha.is_empty() => {
                        previous_base_sha.clone()
                    }
                    _ => self
                        .repo
                        .merge_base(previous_head_sha.clone(), base_target.to_string())?,
                };
                let resolved_base_sha = self
                    .repo
                    .merge_base(head_sha.clone(), base_target.to_string())?;
                let resolved_base_target_sha = self.repo.revparse_single(base_target)?.id();

                if resolved_base_sha != resolved_base_target_sha {
                    println!(
                        "Skipping PR sync authorship rewrite: current PR head is not based on {}",
                        resolved_base_target_sha
                    );
                    return Ok(CiRunResult::SkippedNonRebaseSync);
                }

                if resolved_previous_base_sha == resolved_base_sha {
                    println!(
                        "Skipping PR sync authorship rewrite: PR base did not advance during sync"
                    );
                    return Ok(CiRunResult::SkippedNonRebaseSync);
                }

                if !commit_is_ancestor(&self.repo, &resolved_previous_base_sha, &resolved_base_sha)?
                {
                    println!(
                        "Skipping PR sync authorship rewrite: previous PR base is not an ancestor of current PR base"
                    );
                    return Ok(CiRunResult::SkippedNonRebaseSync);
                }

                let original_commits = commits_in_range_oldest_first(
                    &self.repo,
                    &resolved_previous_base_sha,
                    previous_head_sha,
                    "previous PR",
                )?;
                let new_commits = commits_in_range_oldest_first(
                    &self.repo,
                    &resolved_base_sha,
                    head_sha,
                    "current PR",
                )?;

                println!(
                    "Detected non-fast-forward PR sync: {} original commits -> {} new commits",
                    original_commits.len(),
                    new_commits.len()
                );

                if original_commits.is_empty() || new_commits.is_empty() {
                    println!("No AI authorship to track for this PR rebase (empty commit range)");
                    return Ok(CiRunResult::NoAuthorshipAvailable);
                }

                let notes_before = count_commits_with_authorship_notes(&self.repo, &new_commits);
                if notes_before > 0 {
                    println!(
                        "Skipping PR sync authorship rewrite: {} of {} current PR commits already have authorship notes",
                        notes_before,
                        new_commits.len()
                    );
                    return Ok(CiRunResult::SkippedExistingSyncNotes);
                }

                if !commit_ranges_have_same_patch_ids(&self.repo, &original_commits, &new_commits)?
                {
                    println!(
                        "Skipping PR sync authorship rewrite: previous and current commit ranges are not rebase-equivalent"
                    );
                    return Ok(CiRunResult::SkippedNonRebaseSync);
                }

                println!(
                    "Rewriting authorship for rebased PR head: {} -> {}",
                    previous_head_sha, head_sha
                );

                handle_rewrite_event(
                    &self.repo,
                    RewriteEvent::NonFastForward {
                        old_tip: previous_head_sha.to_string(),
                        new_tip: head_sha.to_string(),
                        onto: Some(resolved_base_sha.clone()),
                    },
                )?;
                println!("Rewrote authorship.");

                let notes_after = count_commits_with_authorship_notes(&self.repo, &new_commits);
                if notes_after == 0 {
                    println!(
                        "No AI authorship to track for this PR rebase (no AI-touched files in PR)"
                    );
                    return Ok(CiRunResult::NoAuthorshipAvailable);
                }

                if options.skip_push {
                    println!("Skipping authorship push (--skip-push). Done.");
                } else {
                    println!("Pushing authorship...");
                    self.repo.push_authorship("origin")?;
                    println!("Pushed authorship. Done.");
                }

                Ok(CiRunResult::SyncAuthorshipRewritten {
                    commit_count: notes_after,
                })
            }
        }
    }

    pub fn teardown(&self) -> Result<(), GitAiError> {
        // Skip cleanup if temp_dir is empty (repository was provided externally)
        if self.temp_dir.as_os_str().is_empty() {
            return Ok(());
        }
        fs::remove_dir_all(self.temp_dir.clone())?;
        Ok(())
    }
}

#[path = "ci_context_patch_id_tests.rs"]
#[cfg(test)]
mod patch_id_tests;

#[path = "ci_context_tests.rs"]
#[cfg(test)]
mod tests;
