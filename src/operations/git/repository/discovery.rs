//! Repository discovery that shells out to git (`git rev-parse`) to resolve the
//! git/common dirs and workdir, plus workspace helpers that group file paths by
//! their containing repository and batch-read blob content at treeishes.

use super::core::Repository;
use super::discovery_no_exec::{
    canonicalize_workdir, no_parent_error, require_dir, worktree_storage_ai_dir,
};
use crate::clients::git_cli::{exec_git, exec_git_stdin};
use crate::error::GitAiError;
use crate::operations::git::cat_file::{batch_path_specs, batch_read_blob_contents};
use crate::operations::git::config_access_retry::with_config_access_retry;
use crate::operations::git::repo_storage::RepoStorage;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Text-dedup constructor for the "Missing --X output from git rev-parse"
/// shape below; Display output is unchanged from the sites it replaces.
fn missing_rev_parse_output(flag: &str) -> GitAiError {
    GitAiError::Generic(format!("Missing --{} output from git rev-parse", flag))
}

pub fn find_repository(global_args: &[String]) -> Result<Repository, GitAiError> {
    let mut rev_parse_args = global_args.to_owned();
    rev_parse_args.push("rev-parse".to_string());
    // Use --git-dir instead of --absolute-git-dir for compatibility with Git < 2.13
    // (--absolute-git-dir was added in Git 2.13; older versions output the literal
    // string "absolute-git-dir" instead of the resolved path).
    rev_parse_args.push("--is-bare-repository".to_string());
    rev_parse_args.push("--git-dir".to_string());
    rev_parse_args.push("--git-common-dir".to_string());

    let rev_parse_output =
        with_config_access_retry(|| exec_git(&rev_parse_args), std::thread::sleep)?;
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
            return Err(missing_rev_parse_output("is-bare-repository"));
        }
    };

    let git_dir_str = lines
        .next()
        .ok_or_else(|| missing_rev_parse_output("git-dir"))?;
    let git_common_dir_str = lines
        .next()
        .ok_or_else(|| missing_rev_parse_output("git-common-dir"))?;
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

    require_dir("Git directory", &git_dir)?;
    require_dir("Git common directory", &git_common_dir)?;

    let workdir = if is_bare {
        git_dir
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| no_parent_error("Git directory", &git_dir))?
    } else {
        let mut top_level_args = global_args.to_owned();
        top_level_args.push("rev-parse".to_string());
        top_level_args.push("--show-toplevel".to_string());
        let output = with_config_access_retry(|| exec_git(&top_level_args), std::thread::sleep)?;
        PathBuf::from(String::from_utf8(output.stdout)?.trim())
    };

    require_dir("Work directory", &workdir)?;

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
    let canonical_workdir = canonicalize_workdir(&workdir)?;

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
        workdir,
        canonical_workdir,
        cached_author_identity: std::sync::OnceLock::new(),
        cached_has_promisor_remote: std::sync::Arc::new(std::sync::OnceLock::new()),
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

pub(crate) fn batch_read_paths_at_treeishes(
    repo: &Repository,
    requests: &[(String, String)],
) -> Result<HashMap<(String, String), String>, GitAiError> {
    let specs = batch_path_specs(repo, requests)?;
    if specs.is_empty() {
        return Ok(HashMap::new());
    }

    let mut args = repo.global_args_for_exec();
    args.extend([
        "cat-file".to_string(),
        "--batch-check=%(objectname) %(objecttype)".to_string(),
    ]);

    let stdin_data = specs
        .iter()
        .map(|(_, spec)| spec.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let output = exec_git_stdin(&args, stdin_data.as_bytes())?;
    let stdout = String::from_utf8(output.stdout)?;
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() != specs.len() {
        return Err(GitAiError::Generic(format!(
            "git cat-file returned {} records for {} path requests",
            lines.len(),
            specs.len()
        )));
    }

    let mut request_blob_oids: HashMap<(String, String), String> = HashMap::new();
    let mut unique_blob_oids = Vec::new();
    let mut seen_blob_oids = HashSet::new();

    for ((request, _), line) in specs.into_iter().zip(lines) {
        // `<oid> blob`. A spec that names nothing is echoed as `<spec> missing`,
        // and the spec may itself hold spaces.
        let Some((oid, "blob")) = line.rsplit_once(' ') else {
            continue;
        };
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
