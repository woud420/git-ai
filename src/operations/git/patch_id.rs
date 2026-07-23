use std::collections::{HashMap, HashSet};

use crate::clients::git_cli::exec_git_stdin;
use crate::error::GitAiError;
use crate::operations::git::repository::Repository;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PatchDiffMode {
    /// Preserve repository-configured diff behavior used by authorship rewrites
    /// and post-commit metrics.
    Configured,
    /// Use the explicit diff behavior used to compare rebased CI commit ranges.
    Canonical,
}

/// Compute stable patch IDs for a batch of commits.
///
/// The returned vector has exactly one entry per input SHA, in the same order.
/// Duplicate inputs therefore produce duplicate outputs. `None` means Git did
/// not emit a patch ID, as is the case for empty commits and merge commits under
/// Git's default combined-diff behavior.
pub(crate) fn stable_patch_ids_for_commits(
    repo: &Repository,
    commit_shas: &[String],
    mode: PatchDiffMode,
) -> Result<Vec<Option<String>>, GitAiError> {
    if commit_shas.is_empty() {
        return Ok(Vec::new());
    }

    let mut seen = HashSet::new();
    let unique_commits: Vec<&str> = commit_shas
        .iter()
        .map(String::as_str)
        .filter(|sha| seen.insert(*sha))
        .collect();

    let mut log_args = repo.global_args_for_exec();
    log_args.extend(
        [
            "log",
            "--stdin",
            "--no-walk",
            "--reverse",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
        ]
        .map(String::from),
    );
    if mode == PatchDiffMode::Canonical {
        log_args.extend(
            [
                "--no-notes",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "--diff-algorithm=default",
                "--indent-heuristic",
            ]
            .map(String::from),
        );
    }
    log_args.extend(["--format=medium".to_string(), "-p".to_string()]);

    let stdin_data = unique_commits.join("\n") + "\n";
    let log_output = exec_git_stdin(&log_args, stdin_data.as_bytes())?;

    let mut patch_args = repo.global_args_for_exec();
    patch_args.extend(["patch-id".to_string(), "--stable".to_string()]);
    let patch_output = exec_git_stdin(&patch_args, &log_output.stdout)?;

    let patch_ids: HashMap<String, String> = String::from_utf8_lossy(&patch_output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let patch_id = parts.next()?;
            let commit_sha = parts.next()?;
            Some((commit_sha.to_string(), patch_id.to_string()))
        })
        .collect();

    Ok(commit_shas
        .iter()
        .map(|sha| patch_ids.get(sha).cloned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::git_cli::exec_git;
    use crate::operations::git::test_utils::TmpRepo;

    fn repo_with_two_patches() -> (TmpRepo, String, String) {
        let repo = TmpRepo::new().expect("tmp repo");
        repo.write_file("one.txt", "one\n", false)
            .expect("write first file");
        let first = repo.commit_all("first").expect("first commit");
        repo.write_file("two.txt", "two\n", false)
            .expect("write second file");
        let second = repo.commit_all("second").expect("second commit");
        (repo, first, second)
    }

    fn repo_with_empty_and_merge() -> (TmpRepo, String, String, String) {
        let repo = TmpRepo::new().expect("tmp repo");
        repo.write_file("base.txt", "base\n", false)
            .expect("write base file");
        let ordinary = repo.commit_all("base").expect("base commit");
        let empty = repo.commit_all("empty").expect("empty commit");

        repo.create_branch("feature").expect("create feature");
        repo.switch_branch("feature").expect("switch to feature");
        repo.write_file("feature.txt", "feature\n", false)
            .expect("write feature file");
        repo.commit_all("feature").expect("feature commit");

        repo.switch_branch("main").expect("switch to main");
        repo.write_file("main.txt", "main\n", false)
            .expect("write main file");
        repo.commit_all("main").expect("main commit");
        repo.git_command(&["merge", "--no-ff", "feature", "-m", "merge"])
            .expect("merge feature");
        let merge = repo
            .git_command(&["rev-parse", "HEAD"])
            .expect("resolve merge")
            .trim()
            .to_string();

        (repo, ordinary, empty, merge)
    }

    fn legacy_canonical_patch_identity(
        repo: &Repository,
        commit_sha: &str,
    ) -> Result<String, GitAiError> {
        let mut show_args = repo.global_args_for_exec();
        show_args.extend(
            [
                "show",
                "--format=",
                "--no-notes",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "--diff-algorithm=default",
                "--indent-heuristic",
                commit_sha,
            ]
            .map(String::from),
        );
        let patch = exec_git(&show_args)?;

        let mut patch_args = repo.global_args_for_exec();
        patch_args.extend(["patch-id".to_string(), "--stable".to_string()]);
        let patch_id_output = exec_git_stdin(&patch_args, &patch.stdout)?;
        Ok(String::from_utf8_lossy(&patch_id_output.stdout)
            .split_whitespace()
            .next()
            .map(str::to_string)
            .unwrap_or_else(|| "empty-patch".to_string()))
    }

    #[test]
    fn preserves_input_order_and_duplicates() {
        let (repo, first, second) = repo_with_two_patches();
        let input = vec![second.clone(), first.clone(), second.clone()];

        let patch_ids =
            stable_patch_ids_for_commits(repo.gitai_repo(), &input, PatchDiffMode::Configured)
                .expect("patch ids");
        let reference = stable_patch_ids_for_commits(
            repo.gitai_repo(),
            &[first, second],
            PatchDiffMode::Configured,
        )
        .expect("reference patch ids");

        assert_eq!(
            patch_ids,
            vec![
                reference[1].clone(),
                reference[0].clone(),
                reference[1].clone(),
            ]
        );
    }

    #[test]
    fn different_patches_have_different_ids() {
        let (repo, first, second) = repo_with_two_patches();
        let patch_ids = stable_patch_ids_for_commits(
            repo.gitai_repo(),
            &[first, second],
            PatchDiffMode::Configured,
        )
        .expect("patch ids");

        assert!(patch_ids[0].is_some());
        assert!(patch_ids[1].is_some());
        assert_ne!(patch_ids[0], patch_ids[1]);
    }

    #[test]
    fn empty_and_merge_commits_have_no_patch_id() {
        let (repo, _, empty, merge) = repo_with_empty_and_merge();

        for mode in [PatchDiffMode::Configured, PatchDiffMode::Canonical] {
            let patch_ids = stable_patch_ids_for_commits(
                repo.gitai_repo(),
                &[empty.clone(), merge.clone()],
                mode,
            )
            .expect("patch ids");
            assert_eq!(patch_ids, vec![None, None]);
        }
    }

    #[test]
    fn canonical_mode_matches_legacy_single_commit_behavior() {
        let (repo, ordinary, empty, merge) = repo_with_empty_and_merge();
        let commits = vec![ordinary, empty, merge];
        let patch_ids =
            stable_patch_ids_for_commits(repo.gitai_repo(), &commits, PatchDiffMode::Canonical)
                .expect("batched patch ids");

        for (commit, patch_id) in commits.iter().zip(patch_ids) {
            let actual = patch_id.unwrap_or_else(|| "empty-patch".to_string());
            let expected = legacy_canonical_patch_identity(repo.gitai_repo(), commit)
                .expect("legacy patch id");
            assert_eq!(actual, expected, "commit {commit}");
        }
    }
}
