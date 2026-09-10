//! Explicit Git/jj path discovery without commands or attribution storage.
//!
//! These constructible `discovery_only` locators are hints, not source identity,
//! ancestry or native-capture evidence. Discovery runs outside a native capture
//! budget; later capture must independently validate paths and payloads under
//! its own descriptor, byte and deadline limits.

use crate::operations::git::repo_state::is_valid_git_dir;
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_METADATA_BYTES: u64 = 16 * 1024;

#[derive(Debug, Serialize)]
pub struct WorkspaceContext {
    pub schema_version: u32,
    pub capability: &'static str,
    pub vcs: &'static str,
    pub workspace_root: PathBuf,
    pub git: GitPaths,
    pub jj: Option<JjPaths>,
    pub colocated: bool,
}

#[derive(Debug, Serialize)]
pub struct GitPaths {
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct JjPaths {
    pub repo_dir: PathBuf,
    pub store_dir: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct ContextError {
    pub code: &'static str,
    pub message: String,
}

impl ContextError {
    pub(crate) fn invalid(path: &Path, message: impl std::fmt::Display) -> Self {
        Self {
            code: "invalid_metadata",
            message: format!("{}: {message}", path.display()),
        }
    }
}

// Explicit callers own jj probing; shared Git discovery also serves Trace2
// ingestion, and constructing Repository would initialize ai storage.
pub fn discover(cwd: &Path) -> Result<WorkspaceContext, ContextError> {
    let cwd = directory(cwd)?;
    for root in cwd.ancestors() {
        let jj_dir = root.join(".jj");
        if entry_exists(&jj_dir)? {
            return discover_jj(root, &jj_dir);
        }
        if let Some(git) = git_at_root(root)? {
            return Ok(WorkspaceContext {
                schema_version: 1,
                capability: "discovery_only",
                vcs: "git",
                workspace_root: root.to_path_buf(),
                git,
                jj: None,
                colocated: false,
            });
        }
    }
    Err(ContextError {
        code: "no_repository",
        message: format!("No Git or jj workspace found from {}", cwd.display()),
    })
}

fn discover_jj(root: &Path, jj_dir: &Path) -> Result<WorkspaceContext, ContextError> {
    let jj_dir = directory(jj_dir)?;
    let working_copy = directory(&jj_dir.join("working_copy"))?;
    require_backend(&working_copy.join("type"), "local")?;
    let repo_path = jj_dir.join("repo");
    let repo_dir = if repo_path.is_dir() {
        directory(&repo_path)?
    } else {
        pointer_directory(&repo_path, &jj_dir)?
    };
    let store_dir = directory(&repo_dir.join("store"))?;
    require_backend(&store_dir.join("type"), "git")?;
    let target = pointer_path(&store_dir.join("git_target"), &store_dir)?;
    let git = git_paths(&target)?;
    let colocated = if let Some(worktree_git) = git_at_root(root)? {
        if worktree_git.git_dir != git.git_dir {
            return Err(ContextError::invalid(
                &repo_dir,
                "jj Git store differs from this workspace's .git directory",
            ));
        }
        true
    } else {
        false
    };
    Ok(WorkspaceContext {
        schema_version: 1,
        capability: "discovery_only",
        vcs: "jj",
        workspace_root: root.to_path_buf(),
        git,
        jj: Some(JjPaths {
            repo_dir,
            store_dir,
        }),
        colocated,
    })
}

fn git_at_root(root: &Path) -> Result<Option<GitPaths>, ContextError> {
    let dot_git = root.join(".git");
    if !entry_exists(&dot_git)? || (dot_git.is_dir() && !is_valid_git_dir(&dot_git)) {
        return Ok(None);
    }
    git_paths(&dot_git).map(Some)
}

fn git_paths(git_dir: &Path) -> Result<GitPaths, ContextError> {
    let metadata = fs::metadata(git_dir).map_err(|error| ContextError::invalid(git_dir, error))?;
    // Resolve only this entry, with bounded reads. The shared worktree helper
    // walks ancestors and could pair a broken nested boundary with an outer repo.
    let target = if metadata.is_file() {
        let content = read_text(git_dir)?;
        let pointer = content
            .strip_prefix("gitdir:")
            .map(str::trim)
            .filter(|pointer| !pointer.is_empty())
            .ok_or_else(|| ContextError::invalid(git_dir, "invalid Git directory pointer"))?;
        git_dir.parent().unwrap_or(git_dir).join(pointer)
    } else {
        git_dir.to_path_buf()
    };
    let git_dir = directory(&target)?;
    if !is_valid_git_dir(&git_dir) {
        return Err(ContextError::invalid(&git_dir, "missing Git HEAD file"));
    }
    // `commondir` is authoritative; an ordinary separate gitdir can also live
    // under a directory named `worktrees` without sharing its parent's store.
    let common_pointer = git_dir.join("commondir");
    let common_dir = if entry_exists(&common_pointer)? {
        let content = read_text(&common_pointer)?;
        let pointer = content.trim();
        if pointer.is_empty() {
            return Err(ContextError::invalid(
                &common_pointer,
                "empty Git common directory pointer",
            ));
        }
        git_dir.join(pointer)
    } else {
        git_dir.clone()
    };
    let common_dir = directory(&common_dir)?;
    directory(&common_dir.join("objects"))?;
    Ok(GitPaths {
        git_dir,
        common_dir,
    })
}

fn entry_exists(path: &Path) -> Result<bool, ContextError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ContextError::invalid(path, error)),
    }
}

fn directory(path: &Path) -> Result<PathBuf, ContextError> {
    let canonical = path
        .canonicalize()
        .map_err(|error| ContextError::invalid(path, error))?;
    if !canonical.is_dir() {
        return Err(ContextError::invalid(path, "expected a directory"));
    }
    Ok(canonical)
}

fn require_backend(path: &Path, expected: &str) -> Result<(), ContextError> {
    if read_text(path)? != expected {
        return Err(ContextError {
            code: "unsupported_backend",
            message: format!("{}: expected the {expected} backend", path.display()),
        });
    }
    Ok(())
}

fn pointer_directory(path: &Path, relative_to: &Path) -> Result<PathBuf, ContextError> {
    directory(&pointer_path(path, relative_to)?)
}

fn pointer_path(path: &Path, relative_to: &Path) -> Result<PathBuf, ContextError> {
    let pointer = read_text(path)?;
    if pointer.is_empty() {
        return Err(ContextError::invalid(path, "empty directory pointer"));
    }
    // jj writes raw paths, so whitespace can be part of the directory name.
    Ok(relative_to.join(pointer))
}

fn read_text(path: &Path) -> Result<String, ContextError> {
    let metadata = fs::metadata(path).map_err(|error| ContextError::invalid(path, error))?;
    if !metadata.is_file() {
        return Err(ContextError::invalid(
            path,
            "expected a regular metadata file",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(MAX_METADATA_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| ContextError::invalid(path, error))?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err(ContextError::invalid(path, "metadata exceeds 16384 bytes"));
    }
    String::from_utf8(bytes).map_err(|error| ContextError {
        code: "unsupported_path_encoding",
        message: format!("{}: metadata is not UTF-8: {error}", path.display()),
    })
}
