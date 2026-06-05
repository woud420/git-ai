use crate::authorship::authorship_log_serialization::AuthorshipLog;
use crate::authorship::rebase_authorship::{
    rewrite_authorship_after_rebase_v2, rewrite_authorship_after_squash_or_rebase,
};
use crate::error::GitAiError;
use crate::git::notes_api::{
    read_authorship_v3 as get_reference_as_authorship_log_v3, read_note as show_authorship_note,
};
use crate::git::refs::{
    AI_AUTHORSHIP_FORK_TRACKING_REF, copy_missing_notes_for_commits_from_ref,
    note_blob_oids_for_commits, ref_exists,
};
use crate::git::repository::{CommitRange, Repository, exec_git};
use crate::git::sync_authorship::fetch_authorship_notes;
use std::fs;
use std::path::PathBuf;

#[cfg(windows)]
const NULL_HOOKS: &str = "NUL";
#[cfg(not(windows))]
const NULL_HOOKS: &str = "/dev/null";

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
}

/// Result of running CiContext
#[derive(Debug)]
pub enum CiRunResult {
    /// Authorship was successfully rewritten for a squash/rebase merge
    AuthorshipRewritten {
        #[allow(dead_code)]
        authorship_log: AuthorshipLog,
    },
    /// Skipped: merge commit has multiple parents (simple merge - authorship already present)
    SkippedSimpleMerge,
    /// Skipped: merge commit equals head (fast-forward - no rewrite needed)
    SkippedFastForward,
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
                head_ref,
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
                match get_reference_as_authorship_log_v3(&self.repo, merge_commit_sha) {
                    Ok(existing_log) => {
                        println!("{} already has authorship", merge_commit_sha);
                        return Ok(CiRunResult::AlreadyExists {
                            authorship_log: existing_log,
                        });
                    }
                    Err(e) => {
                        if show_authorship_note(&self.repo, merge_commit_sha).is_some() {
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
                        let (_source_base, original_commits) =
                            self.original_pr_commits(head_sha, base_ref, base_sha);
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

                // Detect squash vs rebase merge by counting commits
                // For squash: N original commits → 1 merge commit
                // For rebase: N original commits → N rebased commits
                let (original_commits_base, original_commits) =
                    self.original_pr_commits(head_sha, base_ref, base_sha);

                println!(
                    "Original commits in PR: {} (from {:?})",
                    original_commits.len(),
                    original_commits_base
                );

                self.import_fork_notes_for_commits(fork_clone_url, &original_commits, options)?;

                // For multi-commit PRs, check if this is a rebase merge (multiple new commits)
                // by walking back from merge_commit_sha
                if original_commits.len() > 1 {
                    // Try to find the new rebased commits
                    // Walk back from merge_commit_sha the same number of commits as original
                    let mut new_commits =
                        self.get_rebased_commits(merge_commit_sha, original_commits.len());

                    // #1473: on a linear base branch the first-parent walk above can
                    // return pre-existing base commits rather than rebased PR commits,
                    // so a squash merge's count matches a rebase's and gets
                    // misclassified (PR notes then land on unrelated commits). Restrict
                    // to commits the merge actually introduced
                    // (`base_sha..merge_commit_sha`; see gitrevisions(7)) — a squash
                    // yields exactly one, so it can't look like a rebase. GitHub passes
                    // `pull_request.base.sha` and GitLab passes `diff_refs.start_sha`
                    // (the target-branch tip at MR creation); an empty `base_sha`
                    // (transient API failure on either path) safely skips the filter
                    // and falls back to the pre-#1473 behavior.
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

                    if new_commits.len() == original_commits.len() {
                        println!(
                            "Detected rebase merge: {} original -> {} new commits",
                            original_commits.len(),
                            new_commits.len()
                        );
                        // Rebase merge - use v2 which writes authorship to each rebased commit
                        rewrite_authorship_after_rebase_v2(
                            &self.repo,
                            head_sha,
                            &original_commits,
                            &new_commits,
                            "", // human_author not used
                        )?;
                    } else {
                        println!(
                            "Detected squash merge: {} original commits -> 1 merge commit",
                            original_commits.len()
                        );
                        // Squash merge - use existing function which writes to single merge commit
                        rewrite_authorship_after_squash_or_rebase(
                            &self.repo,
                            head_ref,
                            base_ref,
                            head_sha,
                            merge_commit_sha,
                            false,
                        )?;
                    }
                } else {
                    // Single commit - use squash_or_rebase (handles both cases)
                    println!("Single commit PR, using squash/rebase handler");
                    rewrite_authorship_after_squash_or_rebase(
                        &self.repo,
                        head_ref,
                        base_ref,
                        head_sha,
                        merge_commit_sha,
                        false,
                    )?;
                }
                println!("Rewrote authorship.");

                // Check if authorship was created for THIS specific commit
                match get_reference_as_authorship_log_v3(&self.repo, merge_commit_sha) {
                    Ok(authorship_log) => {
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
                        if show_authorship_note(&self.repo, merge_commit_sha).is_some() {
                            return Err(e);
                        }
                        println!(
                            "No AI authorship to track for this commit (no AI-touched files in PR)"
                        );
                        Ok(CiRunResult::NoAuthorshipAvailable)
                    }
                }
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

    /// Fetch authorship notes from a fork repository URL into the fork tracking ref.
    /// Returns Ok(true) if notes were found and fetched,
    /// Ok(false) if no notes exist on the fork.
    fn fetch_fork_notes(repo: &Repository, fork_url: &str) -> Result<bool, GitAiError> {
        let tracking_ref = AI_AUTHORSHIP_FORK_TRACKING_REF;

        // Check if the fork has notes
        let mut ls_remote_args = repo.global_args_for_exec();
        ls_remote_args.push("ls-remote".to_string());
        ls_remote_args.push(fork_url.to_string());
        ls_remote_args.push("refs/notes/ai".to_string());

        match exec_git(&ls_remote_args) {
            Ok(output) => {
                let result = String::from_utf8_lossy(&output.stdout).to_string();
                if result.trim().is_empty() {
                    return Ok(false);
                }
            }
            Err(e) => {
                return Err(GitAiError::Generic(format!(
                    "Failed to check fork for authorship notes: {}",
                    e
                )));
            }
        }

        // Fetch notes from the fork URL into a tracking ref
        let fetch_refspec = format!("+refs/notes/ai:{}", tracking_ref);
        let mut fetch_args = repo.global_args_for_exec();
        fetch_args.push("-c".to_string());
        fetch_args.push(format!("core.hooksPath={}", NULL_HOOKS));
        fetch_args.push("fetch".to_string());
        fetch_args.push("--no-tags".to_string());
        fetch_args.push("--recurse-submodules=no".to_string());
        fetch_args.push("--no-write-fetch-head".to_string());
        fetch_args.push("--no-write-commit-graph".to_string());
        fetch_args.push("--no-auto-maintenance".to_string());
        fetch_args.push(fork_url.to_string());
        fetch_args.push(fetch_refspec);

        exec_git(&fetch_args)?;

        Ok(true)
    }

    fn import_fork_notes_for_commits(
        &self,
        fork_clone_url: &Option<String>,
        commit_shas: &[String],
        options: CiRunOptions,
    ) -> Result<usize, GitAiError> {
        let Some(fork_url) = fork_clone_url else {
            return Ok(0);
        };
        if commit_shas.is_empty() {
            println!("No PR commits found; skipping fork authorship note import");
            return Ok(0);
        }

        let source_ref_available = if options.skip_fetch_fork_notes {
            println!(
                "Skipping fork authorship notes fetch; using {} if it exists",
                AI_AUTHORSHIP_FORK_TRACKING_REF
            );
            ref_exists(&self.repo, AI_AUTHORSHIP_FORK_TRACKING_REF)
        } else {
            println!(
                "Fetching authorship notes from fork into {}...",
                AI_AUTHORSHIP_FORK_TRACKING_REF
            );
            match Self::fetch_fork_notes(&self.repo, fork_url) {
                Ok(true) => {
                    println!("Fetched authorship notes from fork");
                    true
                }
                Ok(false) => {
                    println!("No authorship notes found on fork");
                    false
                }
                Err(e) => {
                    println!(
                        "Warning: Failed to fetch fork notes ({}), continuing without them",
                        e
                    );
                    false
                }
            }
        };

        if !source_ref_available {
            return Ok(0);
        }

        let copied = copy_missing_notes_for_commits_from_ref(
            &self.repo,
            AI_AUTHORSHIP_FORK_TRACKING_REF,
            commit_shas,
        )?;
        println!(
            "Imported {} fork authorship notes for {} PR commits from {}",
            copied,
            commit_shas.len(),
            AI_AUTHORSHIP_FORK_TRACKING_REF
        );
        Ok(copied)
    }

    fn has_notes_for_any_commit(&self, commit_shas: &[String]) -> Result<bool, GitAiError> {
        Ok(!note_blob_oids_for_commits(&self.repo, commit_shas)?.is_empty())
    }

    fn original_pr_commits(
        &self,
        head_sha: &str,
        base_ref: &str,
        base_sha: &str,
    ) -> (Option<String>, Vec<String>) {
        if !base_sha.is_empty()
            && let Ok(mut commits) = CommitRange::new_infer_refname(
                &self.repo,
                base_sha.to_string(),
                head_sha.to_string(),
                None,
            )
            .map(|r| r.all_commits())
            && !commits.is_empty()
        {
            commits.reverse();
            return (Some(format!("base_sha {}", base_sha)), commits);
        }

        let merge_base = self
            .repo
            .merge_base(head_sha.to_string(), base_ref.to_string())
            .ok();

        if let Some(ref base) = merge_base
            && let Ok(mut commits) =
                CommitRange::new_infer_refname(&self.repo, base.clone(), head_sha.to_string(), None)
                    .map(|r| r.all_commits())
            && !commits.is_empty()
        {
            commits.reverse();
            return (Some(format!("merge-base {}", base)), commits);
        }

        let resolved_head = self
            .repo
            .revparse_single(head_sha)
            .map(|obj| obj.id())
            .unwrap_or_else(|_| head_sha.to_string());
        (
            merge_base.map(|base| format!("merge-base {}", base)),
            vec![resolved_head],
        )
    }

    /// Get the rebased commits by walking back from merge_commit_sha.
    /// For a rebase merge with N original commits, there should be N new commits
    /// ending at merge_commit_sha.
    #[doc(hidden)]
    pub fn get_rebased_commits(
        &self,
        merge_commit_sha: &str,
        expected_count: usize,
    ) -> Vec<String> {
        let mut commits = Vec::new();
        // Resolve to a full SHA up front so the entries are comparable to the
        // full 40-char SHAs produced by `git rev-list` (the #1473 `retain` filter
        // in `run_with_options` compares against such a set). Callers like
        // `git-ai ci local merge` may pass an abbreviated `merge_commit_sha`; the
        // remaining entries already come from parent ids, which are full.
        let mut current_sha = self
            .repo
            .revparse_single(merge_commit_sha)
            .map(|obj| obj.id())
            .unwrap_or_else(|_| merge_commit_sha.to_string());

        for _ in 0..expected_count {
            commits.push(current_sha.clone());

            // Get the parent of current commit
            match self.repo.find_commit(current_sha.clone()) {
                Ok(commit) => {
                    let parents: Vec<_> = commit.parents().collect();
                    if parents.len() != 1 {
                        // Not a linear chain (merge commit or root), stop here
                        break;
                    }
                    current_sha = parents[0].id().to_string();
                }
                Err(_) => break,
            }
        }

        // Reverse to get oldest-to-newest order (same as original_commits)
        commits.reverse();
        commits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ci_event_debug() {
        let event = CiEvent::Merge {
            merge_commit_sha: "abc123".to_string(),
            head_ref: "feature".to_string(),
            head_sha: "def456".to_string(),
            base_ref: "main".to_string(),
            base_sha: "ghi789".to_string(),
            fork_clone_url: None,
        };

        let debug_str = format!("{:?}", event);
        assert!(debug_str.contains("Merge"));
        assert!(debug_str.contains("abc123"));
        assert!(debug_str.contains("feature"));
    }

    #[test]
    fn test_ci_run_result_debug() {
        let result = CiRunResult::SkippedSimpleMerge;
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("SkippedSimpleMerge"));

        let result2 = CiRunResult::SkippedFastForward;
        let debug_str2 = format!("{:?}", result2);
        assert!(debug_str2.contains("SkippedFastForward"));

        let result3 = CiRunResult::NoAuthorshipAvailable;
        let debug_str3 = format!("{:?}", result3);
        assert!(debug_str3.contains("NoAuthorshipAvailable"));
    }
}
