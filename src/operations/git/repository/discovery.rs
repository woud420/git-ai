//! Repository discovery that shells out to git (`git rev-parse`) to resolve the
//! git/common dirs and workdir, plus workspace helpers that group file paths by
//! their containing repository and batch-read blob content at treeishes.

use super::core::Repository;
use super::discovery_no_exec::{
    canonicalize_workdir, no_parent_error, require_dir, worktree_storage_ai_dir,
};
use crate::clients::git_cli::{exec_git, exec_git_allow_nonzero};
use crate::error::GitAiError;
use crate::operations::git::cat_file::{batch_path_specs, batch_read_blobs_by_name};
use crate::operations::git::config_access_retry::with_config_access_retry;
use crate::operations::git::repo_storage::RepoStorage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Text-dedup constructor for the "Missing --X output from git rev-parse"
/// shape below; Display output is unchanged from the sites it replaces.
fn missing_rev_parse_output(flag: &str) -> GitAiError {
    GitAiError::Generic(format!("Missing --{} output from git rev-parse", flag))
}

/// `(is_bare, git_dir, git_common_dir, show_toplevel)`. `show_toplevel` is
/// `Some` only when the combined `rev-parse` answered it for a work tree.
type RevParseDiscovery = (bool, String, String, Option<String>);

fn rev_parse_args(global_args: &[String], flags: &[&str]) -> Vec<String> {
    let mut args = global_args.to_owned();
    args.push("rev-parse".to_string());
    args.extend(flags.iter().map(|flag| flag.to_string()));
    args
}

fn non_empty_lines(stdout: &str) -> impl Iterator<Item = &str> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
}

/// Parses `--is-bare-repository`, `--git-dir` and `--git-common-dir` output, in
/// that order, from the front of `lines`.
fn parse_discovery_head<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
) -> Result<(bool, String, String), GitAiError> {
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
    let git_dir = lines
        .next()
        .ok_or_else(|| missing_rev_parse_output("git-dir"))?;
    let git_common_dir = lines
        .next()
        .ok_or_else(|| missing_rev_parse_output("git-common-dir"))?;
    Ok((is_bare, git_dir.to_string(), git_common_dir.to_string()))
}

// Use --git-dir instead of --absolute-git-dir for compatibility with Git < 2.13
// (--absolute-git-dir was added in Git 2.13; older versions output the literal
// string "absolute-git-dir" instead of the resolved path).
const DISCOVERY_FLAGS: [&str; 3] = ["--is-bare-repository", "--git-dir", "--git-common-dir"];

/// Discovery in one `git rev-parse` that also asks for `--show-toplevel`.
/// Git refuses that last flag outside a work tree, after printing the answers
/// to the flags before it. A bare repository keeps those answers, exactly what
/// the three-flag call prints; any other failure returns `None`, and
/// `find_repository` falls back to the original calls, keeping their results
/// and errors.
fn rev_parse_discovery_with_toplevel(global_args: &[String]) -> Option<RevParseDiscovery> {
    let mut flags = DISCOVERY_FLAGS.to_vec();
    flags.push("--show-toplevel");
    let output = exec_git_allow_nonzero(&rev_parse_args(global_args, &flags)).ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;
    let mut lines = non_empty_lines(&stdout);
    let (is_bare, git_dir, git_common_dir) = parse_discovery_head(&mut lines).ok()?;
    if is_bare {
        return Some((true, git_dir, git_common_dir, None));
    }
    if !output.status.success() {
        return None;
    }
    // Used exactly as the separate call would give it: trimmed, and empty
    // when Git printed nothing.
    let top_level = lines.next().unwrap_or_default().to_string();
    Some((false, git_dir, git_common_dir, Some(top_level)))
}

fn rev_parse_discovery(global_args: &[String]) -> Result<RevParseDiscovery, GitAiError> {
    let args = rev_parse_args(global_args, &DISCOVERY_FLAGS);
    let output = with_config_access_retry(|| exec_git(&args), std::thread::sleep)?;
    let stdout = String::from_utf8(output.stdout)?;
    let (is_bare, git_dir, git_common_dir) = parse_discovery_head(&mut non_empty_lines(&stdout))?;
    Ok((is_bare, git_dir, git_common_dir, None))
}

pub fn find_repository(global_args: &[String]) -> Result<Repository, GitAiError> {
    let (is_bare, git_dir_str, git_common_dir_str, top_level) =
        match rev_parse_discovery_with_toplevel(global_args) {
            Some(found) => found,
            None => rev_parse_discovery(global_args)?,
        };
    let command_base_dir = resolve_command_base_dir(global_args)?;
    let git_dir = if Path::new(&git_dir_str).is_relative() {
        command_base_dir.join(&git_dir_str)
    } else {
        PathBuf::from(&git_dir_str)
    };
    let git_common_dir = if Path::new(&git_common_dir_str).is_relative() {
        command_base_dir.join(&git_common_dir_str)
    } else {
        PathBuf::from(&git_common_dir_str)
    };

    require_dir("Git directory", &git_dir)?;
    require_dir("Git common directory", &git_common_dir)?;

    let workdir = if is_bare {
        git_dir
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| no_parent_error("Git directory", &git_dir))?
    } else if let Some(top_level) = top_level {
        PathBuf::from(top_level)
    } else {
        let top_level_args = rev_parse_args(global_args, &["--show-toplevel"]);
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
        discovered_is_bare: Some(is_bare),
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
    let names: Vec<String> = specs.iter().map(|(_, spec)| spec.clone()).collect();
    let blobs = batch_read_blobs_by_name(repo, &names)?;
    Ok(specs
        .into_iter()
        .zip(blobs)
        .filter_map(|((request, _), blob)| Some((request.clone(), blob?)))
        .collect())
}
