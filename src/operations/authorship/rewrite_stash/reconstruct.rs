use crate::clients::git_cli::{disable_internal_git_hooks, exec_git_allow_nonzero_with_env};
use crate::error::GitAiError;
use crate::operations::git::repository::{Repository, batch_read_paths_at_treeishes};
use std::collections::HashMap;
use std::fs;

pub(super) fn reconstruct_stash_applied_contents(
    repo: &Repository,
    stash_sha: &str,
    target_head: &str,
    file_paths: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    if file_paths.is_empty() {
        return Ok(HashMap::new());
    }

    let unique = format!(
        "git-ai-stash-apply-{}-{}",
        std::process::id(),
        crate::model::clock::now_nanos()
    );
    let temp_dir = std::env::temp_dir().join(unique);
    let index_path = temp_dir.join("index");
    let worktree_path = temp_dir.join("worktree");
    fs::create_dir_all(&worktree_path)?;

    let result = (|| {
        let _guard = disable_internal_git_hooks();
        run_isolated_git(
            repo,
            vec!["read-tree".to_string(), target_head.to_string()],
            &index_path,
            &worktree_path,
            true,
        )?;
        // `-f` (force) is required: on case-insensitive filesystems (macOS/Windows)
        // a tree with case-colliding paths (e.g. `README.md` and `readme.md`) makes
        // `checkout-index -a` fail with "already exists, no checkout" for the second
        // entry. This is a throwaway scratch worktree, so last-casing-wins is harmless.
        run_isolated_git(
            repo,
            vec![
                "checkout-index".to_string(),
                "-a".to_string(),
                "-f".to_string(),
            ],
            &index_path,
            &worktree_path,
            true,
        )?;
        let _ = run_isolated_git(
            repo,
            vec![
                "stash".to_string(),
                "apply".to_string(),
                stash_sha.to_string(),
            ],
            &index_path,
            &worktree_path,
            false,
        )?;
        run_isolated_git(
            repo,
            vec!["add".to_string(), "-A".to_string()],
            &index_path,
            &worktree_path,
            true,
        )?;
        let output = run_isolated_git(
            repo,
            vec!["write-tree".to_string()],
            &index_path,
            &worktree_path,
            true,
        )?;
        let result_tree = String::from_utf8(output.stdout)?.trim().to_string();
        let requests: Vec<(String, String)> = file_paths
            .iter()
            .map(|path| (result_tree.clone(), path.clone()))
            .collect();
        let contents = batch_read_paths_at_treeishes(repo, &requests)?;
        Ok(contents
            .into_iter()
            .map(|((_, path), content)| (path, content))
            .collect())
    })();

    let _ = fs::remove_dir_all(&temp_dir);
    result
}

fn run_isolated_git(
    repo: &Repository,
    args: Vec<String>,
    index_path: &std::path::Path,
    worktree_path: &std::path::Path,
    require_success: bool,
) -> Result<std::process::Output, GitAiError> {
    let mut full_args = repo.global_args_for_exec();
    full_args.extend(args);
    let envs = [
        ("GIT_INDEX_FILE", index_path.as_os_str()),
        ("GIT_WORK_TREE", worktree_path.as_os_str()),
    ];
    let output = exec_git_allow_nonzero_with_env(&full_args, &envs)?;
    if require_success && !output.status.success() {
        return Err(GitAiError::GitCliError {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            args: full_args,
        });
    }
    Ok(output)
}
