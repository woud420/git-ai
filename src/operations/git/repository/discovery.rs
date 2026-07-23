//! Repository discovery that shells out to git (`git rev-parse`) to resolve the
//! git/common dirs and workdir, plus workspace helpers that group file paths by
//! their containing repository and batch-read blob content at treeishes.

use super::core::Repository;
use super::discovery_no_exec::worktree_storage_ai_dir;
use crate::clients::git_cli::{exec_git, exec_git_stdin};
use crate::error::GitAiError;
use crate::operations::git::cat_file::batch_read_blob_contents;
use crate::operations::git::repo_storage::RepoStorage;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub fn find_repository(global_args: &[String]) -> Result<Repository, GitAiError> {
    let mut rev_parse_args = global_args.to_owned();
    rev_parse_args.push("rev-parse".to_string());
    // Use --git-dir instead of --absolute-git-dir for compatibility with Git < 2.13
    // (--absolute-git-dir was added in Git 2.13; older versions output the literal
    // string "absolute-git-dir" instead of the resolved path).
    rev_parse_args.push("--is-bare-repository".to_string());
    rev_parse_args.push("--git-dir".to_string());
    rev_parse_args.push("--git-common-dir".to_string());

    let rev_parse_output = exec_git(&rev_parse_args)?;
    let rev_parse_stdout = String::from_utf8(rev_parse_output.stdout)?;
    let mut lines = rev_parse_stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());

    let is_bare = match lines.next() {
        Some("true") => true,
        Some("false") => false,
        Some(other) => {
            return Err(GitAiError::Generic(format!(
                "Unexpected --is-bare-repository output: {}",
                other
            )));
        }
        None => {
            return Err(GitAiError::Generic(
                "Missing --is-bare-repository output from git rev-parse".to_string(),
            ));
        }
    };

    let git_dir_str = lines.next().ok_or_else(|| {
        GitAiError::Generic("Missing --git-dir output from git rev-parse".to_string())
    })?;
    let git_common_dir_str = lines.next().ok_or_else(|| {
        GitAiError::Generic("Missing --git-common-dir output from git rev-parse".to_string())
    })?;
    let command_base_dir = resolve_command_base_dir(global_args)?;
    let git_dir = if Path::new(git_dir_str).is_relative() {
        command_base_dir.join(git_dir_str)
    } else {
        PathBuf::from(git_dir_str)
    };
    let git_common_dir = if Path::new(git_common_dir_str).is_relative() {
        command_base_dir.join(git_common_dir_str)
    } else {
        PathBuf::from(git_common_dir_str)
    };

    if !git_dir.is_dir() {
        return Err(GitAiError::Generic(format!(
            "Git directory does not exist: {}",
            git_dir.display()
        )));
    }
    if !git_common_dir.is_dir() {
        return Err(GitAiError::Generic(format!(
            "Git common directory does not exist: {}",
            git_common_dir.display()
        )));
    }

    let workdir = if is_bare {
        git_dir.parent().map(Path::to_path_buf).ok_or_else(|| {
            GitAiError::Generic(format!(
                "Git directory has no parent: {}",
                git_dir.display()
            ))
        })?
    } else {
        let mut top_level_args = global_args.to_owned();
        top_level_args.push("rev-parse".to_string());
        top_level_args.push("--show-toplevel".to_string());
        let output = exec_git(&top_level_args)?;
        PathBuf::from(String::from_utf8(output.stdout)?.trim())
    };

    if !workdir.is_dir() {
        return Err(GitAiError::Generic(format!(
            "Work directory does not exist: {}",
            workdir.display()
        )));
    }

    // Ensure all internal git commands use a stable repository root consistently.
    let mut normalized_global_args = global_args.to_owned();
    let command_root = if is_bare {
        git_dir.display().to_string()
    } else {
        workdir.display().to_string()
    };

    if normalized_global_args.is_empty() {
        normalized_global_args = vec!["-C".to_string(), command_root];
    } else if normalized_global_args.len() == 2
        && normalized_global_args[0] == "-C"
        && normalized_global_args[1] != command_root
    {
        normalized_global_args[1] = command_root;
    }

    // Canonicalize workdir for reliable path comparisons (especially on Windows)
    // On Windows, canonical paths use the \\?\ UNC prefix, which makes path.starts_with()
    // comparisons work correctly. We store both regular and canonical versions.
    let canonical_workdir = workdir.canonicalize().map_err(|e| {
        GitAiError::Generic(format!(
            "Failed to canonicalize working directory {}: {}",
            workdir.display(),
            e
        ))
    })?;

    let worktree_ai_dir = worktree_storage_ai_dir(&git_dir, &git_common_dir);
    let storage = if worktree_ai_dir == git_dir.join("ai") {
        RepoStorage::for_repo_path(&git_dir, &workdir)?
    } else {
        RepoStorage::for_isolated_worktree_storage(&worktree_ai_dir, &workdir)?
    };

    Ok(Repository {
        global_args: normalized_global_args,
        storage,
        git_dir,
        git_common_dir,
        pre_command_base_commit: None,
        pre_command_refname: None,
        pre_reset_target_commit: None,
        pre_update_ref_refname: None,
        pre_update_ref_old_target: None,
        pre_update_ref_affects_checked_out_branch: None,
        workdir,
        canonical_workdir,
        cached_author_identity: std::sync::OnceLock::new(),
    })
}

#[doc(hidden)]
pub fn resolve_command_base_dir(global_args: &[String]) -> Result<PathBuf, GitAiError> {
    let mut base: Option<PathBuf> = None;
    let mut idx = 0usize;

    while idx < global_args.len() {
        if global_args[idx] == "-C" {
            let path_arg = global_args.get(idx + 1).ok_or_else(|| {
                GitAiError::Generic("Missing path after -C in global git args".to_string())
            })?;

            let next_base = PathBuf::from(path_arg);
            base = Some(if next_base.is_absolute() {
                next_base
            } else {
                let current = match &base {
                    Some(existing) => existing.clone(),
                    None => std::env::current_dir().map_err(GitAiError::IoError)?,
                };
                current.join(next_base)
            });
            idx += 2;
            continue;
        }
        idx += 1;
    }

    match base {
        Some(base) => Ok(base),
        None => std::env::current_dir().map_err(GitAiError::IoError),
    }
}

pub fn find_repository_in_path(path: &str) -> Result<Repository, GitAiError> {
    let global_args = vec!["-C".to_string(), path.to_string()];
    find_repository(&global_args)
}

/// Find the git repository that contains the given file path by walking up the directory tree.
///
/// This function is useful when working with multi-repository workspaces where the workspace
/// root itself may not be a git repository, but contains multiple independent git repositories.
///
/// # Arguments
///  * `file_path` - Absolute path to a file
///  * `workspace_root` - Optional workspace root path. If provided, the search will stop at this
///    boundary to avoid finding repositories outside the workspace.
///
/// # Returns
/// * `Ok(Repository)` - The repository containing the file
/// * `Err(GitAiError)` - If no repository is found or other errors occur
pub fn find_repository_for_file(
    file_path: &str,
    workspace_root: Option<&str>,
) -> Result<Repository, GitAiError> {
    let file_path = PathBuf::from(file_path);

    // Get the directory containing the file (or the path itself if it's a directory)
    let start_dir = if file_path.is_dir() {
        file_path.clone()
    } else {
        file_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| file_path.clone())
    };

    // Canonicalize paths for consistent comparison
    let start_dir = start_dir
        .canonicalize()
        .unwrap_or_else(|_| start_dir.clone());

    let workspace_boundary = workspace_root.map(|root| {
        PathBuf::from(root)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(root))
    });

    // Walk up the directory tree looking for a .git directory
    let mut current_dir = Some(start_dir.as_path());

    while let Some(dir) = current_dir {
        // Check if we've reached the workspace boundary
        if let Some(ref boundary) = workspace_boundary {
            // Stop if we've gone above the workspace root
            if !dir.starts_with(boundary) && dir != boundary.as_path() {
                break;
            }
        }

        // Check for .git directory or file (file for submodules/worktrees)
        let git_path = dir.join(".git");
        if git_path.exists() {
            // Found a .git - but we need to check if this is a submodule
            // Submodules have a .git file (not directory) that points to the parent's .git/modules
            if git_path.is_file() {
                // This is a submodule - read the file to check if it points to modules/
                if let Ok(content) = std::fs::read_to_string(&git_path)
                    && content.contains("gitdir:")
                    && content.contains("/modules/")
                {
                    // This is a submodule, skip it and continue searching up
                    current_dir = dir.parent();
                    continue;
                }
            }

            // Found a real git repository, use find_repository_in_path
            return find_repository_in_path(&dir.to_string_lossy());
        }

        current_dir = dir.parent();
    }

    Err(GitAiError::Generic(format!(
        "No git repository found for file: {}",
        file_path.display()
    )))
}

/// Group edited file paths by their containing git repository.
///
/// This function takes a list of file paths and groups them by the git repository
/// they belong to. Files that don't belong to any repository are collected separately.
///
/// # Arguments
/// * `file_paths` - List of absolute file paths to group
/// * `workspace_root` - Optional workspace root to limit repository detection
///
/// # Returns
/// A tuple of:
/// * `HashMap<PathBuf, (Repository, Vec<String>)>` - Map of repo root to (repo, file paths)
/// * `Vec<String>` - Files that couldn't be associated with any repository
#[allow(clippy::type_complexity)]
pub fn group_files_by_repository(
    file_paths: &[String],
    workspace_root: Option<&str>,
) -> (HashMap<PathBuf, (Repository, Vec<String>)>, Vec<String>) {
    let mut repo_files: HashMap<PathBuf, (Repository, Vec<String>)> = HashMap::new();
    let mut orphan_files: Vec<String> = Vec::new();

    for file_path in file_paths {
        match find_repository_for_file(file_path, workspace_root) {
            Ok(repo) => {
                let workdir = match repo.workdir() {
                    Ok(dir) => dir,
                    Err(_) => {
                        orphan_files.push(file_path.clone());
                        continue;
                    }
                };

                repo_files
                    .entry(workdir.clone())
                    .or_insert_with(|| (repo, Vec::new()))
                    .1
                    .push(file_path.clone());
            }
            Err(_) => {
                orphan_files.push(file_path.clone());
            }
        }
    }

    (repo_files, orphan_files)
}

pub(crate) fn batch_read_paths_at_treeishes(
    repo: &Repository,
    requests: &[(String, String)],
) -> Result<HashMap<(String, String), String>, GitAiError> {
    if requests.is_empty() {
        return Ok(HashMap::new());
    }

    let mut args = repo.global_args_for_exec();
    args.extend([
        "cat-file".to_string(),
        "--batch-check=%(objectname) %(objecttype)".to_string(),
    ]);

    let stdin_data = requests
        .iter()
        .map(|(treeish, path)| format!("{treeish}:{path}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let output = exec_git_stdin(&args, stdin_data.as_bytes())?;
    let stdout = String::from_utf8(output.stdout)?;
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() != requests.len() {
        return Err(GitAiError::Generic(format!(
            "git cat-file returned {} records for {} path requests",
            lines.len(),
            requests.len()
        )));
    }

    let mut request_blob_oids: HashMap<(String, String), String> = HashMap::new();
    let mut unique_blob_oids = Vec::new();
    let mut seen_blob_oids = HashSet::new();

    for (request, line) in requests.iter().zip(lines) {
        let mut parts = line.split_whitespace();
        let Some(oid) = parts.next() else {
            continue;
        };
        if parts.next() != Some("blob") {
            continue;
        }
        let oid = oid.to_string();
        request_blob_oids.insert(request.clone(), oid.clone());
        if seen_blob_oids.insert(oid.clone()) {
            unique_blob_oids.push(oid);
        }
    }

    let blob_contents = batch_read_blob_contents(repo, &unique_blob_oids)?;

    let mut contents = HashMap::new();
    for (request, blob_oid) in request_blob_oids {
        if let Some(content) = blob_contents.get(&blob_oid) {
            contents.insert(request, content.clone());
        }
    }
    Ok(contents)
}
