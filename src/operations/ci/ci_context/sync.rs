use super::{CiContext, CiRunOptions};
use crate::clients::git_cli::{exec_git, exec_git_allow_nonzero};
use crate::error::GitAiError;
use crate::operations::git::notes_api::read_note;
use crate::operations::git::patch_id::{PatchDiffMode, stable_patch_ids_for_commits};
use crate::operations::git::refs::{
    AI_AUTHORSHIP_FORK_TRACKING_REF, copy_missing_notes_for_commits_from_ref, ref_exists,
};
use crate::operations::git::repository::{CommitRange, Repository};

#[cfg(windows)]
const NULL_HOOKS: &str = "NUL";
#[cfg(not(windows))]
const NULL_HOOKS: &str = "/dev/null";

impl CiContext {
    /// Fetch authorship notes from a fork repository URL into the fork tracking ref.
    /// Returns Ok(true) if notes were found and fetched,
    /// Ok(false) if no notes exist on the fork.
    pub(super) fn fetch_fork_notes(repo: &Repository, fork_url: &str) -> Result<bool, GitAiError> {
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

    pub(super) fn import_fork_notes_for_commits(
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

    pub(super) fn has_notes_for_any_commit(
        &self,
        commit_shas: &[String],
    ) -> Result<bool, GitAiError> {
        // Backend-aware: on the HTTP backend notes live in the notes-db cache,
        // so a refs/notes/ai-only check would always report "no notes".
        Ok(
            !crate::operations::git::notes_api::commits_with_notes(&self.repo, commit_shas)?
                .is_empty(),
        )
    }

    pub(super) fn original_pr_commits(
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

    pub(super) fn original_pr_commits_for_merge_commit(
        &self,
        merge_commit: &crate::operations::git::repository::Commit<'_>,
        head_sha: &str,
        base_ref: &str,
        base_sha: &str,
    ) -> (Option<String>, Vec<String>) {
        if let Ok(first_parent) = merge_commit.parent(0) {
            let first_parent_sha = first_parent.id();
            if let Ok(mut commits) = CommitRange::new_infer_refname(
                &self.repo,
                first_parent_sha.clone(),
                head_sha.to_string(),
                None,
            )
            .map(|r| r.all_commits())
                && !commits.is_empty()
            {
                commits.reverse();
                return (
                    Some(format!("merge first-parent {}", first_parent_sha)),
                    commits,
                );
            }
        }

        self.original_pr_commits(head_sha, base_ref, base_sha)
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
        // full 40-char SHAs produced by `git rev-list`. Callers like
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

pub(super) fn commits_in_range_oldest_first(
    repo: &Repository,
    start_sha: &str,
    end_sha: &str,
    label: &str,
) -> Result<Vec<String>, GitAiError> {
    if start_sha == end_sha {
        return Ok(Vec::new());
    }

    let mut commits =
        CommitRange::new_infer_refname(repo, start_sha.to_string(), end_sha.to_string(), None)
            .map_err(|e| {
                GitAiError::Generic(format!(
                    "Failed to resolve {} commit range {}..{}: {}",
                    label, start_sha, end_sha, e
                ))
            })?
            .all_commits();

    commits.reverse();
    Ok(commits)
}

pub(super) fn count_commits_with_authorship_notes(repo: &Repository, commits: &[String]) -> usize {
    commits
        .iter()
        .filter(|sha| read_note(repo, sha).is_some())
        .count()
}

pub(super) fn ensure_commit_available_for_sync(
    repo: &Repository,
    commit_sha: &str,
    fetch_remote: &str,
    fetch_ref: &str,
    skip_fetch: bool,
) -> Result<(), GitAiError> {
    let commit_spec = format!("{}^{{commit}}", commit_sha);
    if repo.revparse_single(&commit_spec).is_ok() {
        return Ok(());
    }
    if skip_fetch {
        return Err(GitAiError::Generic(format!(
            "Commit {} is not available locally and sync ref fetch is disabled",
            commit_sha
        )));
    }

    println!("Fetching PR sync commit {} into {}", commit_sha, fetch_ref);
    let mut args = repo.global_args_for_exec();
    args.push("fetch".to_string());
    if sync_fetch_remote_supports_lazy_blobs(repo, fetch_remote)? {
        args.push("--filter=blob:none".to_string());
    }
    args.push("--no-tags".to_string());
    args.push(fetch_remote.to_string());
    args.push(format!("{}:{}", commit_sha, fetch_ref));
    exec_git(&args)?;
    repo.revparse_single(&commit_spec)?;
    Ok(())
}

pub(super) fn sync_fetch_remote_supports_lazy_blobs(
    repo: &Repository,
    fetch_remote: &str,
) -> Result<bool, GitAiError> {
    if fetch_remote.contains("://") || fetch_remote.contains('@') {
        return Ok(false);
    }

    let mut args = repo.global_args_for_exec();
    args.push("config".to_string());
    args.push("--bool".to_string());
    args.push("--get".to_string());
    args.push(format!("remote.{}.promisor", fetch_remote));

    let output = exec_git_allow_nonzero(&args)?;
    match output.status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&output.stdout).trim() == "true"),
        Some(1) => Ok(false),
        code => Err(GitAiError::GitCliError {
            code,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            args,
        }),
    }
}

pub(super) fn commit_ranges_have_same_patch_ids(
    repo: &Repository,
    original_commits: &[String],
    new_commits: &[String],
) -> Result<bool, GitAiError> {
    if original_commits.len() != new_commits.len() {
        return Ok(false);
    }

    let mut commits = Vec::with_capacity(original_commits.len() + new_commits.len());
    commits.extend_from_slice(original_commits);
    commits.extend_from_slice(new_commits);
    let patch_ids = stable_patch_ids_for_commits(repo, &commits, PatchDiffMode::Canonical)?;
    let (original_patch_ids, new_patch_ids) = patch_ids.split_at(original_commits.len());
    Ok(original_patch_ids == new_patch_ids)
}

pub(super) fn commit_is_ancestor(
    repo: &Repository,
    ancestor_sha: &str,
    descendant_sha: &str,
) -> Result<bool, GitAiError> {
    let ancestor = repo.revparse_single(ancestor_sha)?.id();
    let descendant = repo.revparse_single(descendant_sha)?.id();
    repo.is_ancestor(&ancestor, &descendant)
}
