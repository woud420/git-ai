//! The [`Repository`] struct and its core methods: config access, identity
//! resolution, ref/HEAD lookup, object lookup, and basic git plumbing. Diff and
//! bulk object-read methods live in the sibling `diff` and `object_reads`
//! modules; both extend this same `impl Repository`.

use super::commits::{Commit, Object};
use super::discovery_no_exec::git_config_file_for_repo_paths;
use super::git_objects::{Blob, Reference};
use super::identity::{
    GitAuthorIdentity, GitIdentityResolution, git_config_identity_resolution_from_config,
    resolve_git_var_identity_with_args,
};
use crate::clients::git_cli::{exec_git, exec_git_allow_nonzero};
use crate::config;
use crate::error::GitAiError;
use crate::operations::git::repo_storage::RepoStorage;
use crate::operations::git::sync_authorship::push_authorship_notes;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use super::diff::probe_configured_git_version;

#[derive(Debug, Clone)]
pub struct Repository {
    pub(super) global_args: Vec<String>,
    pub(super) git_dir: PathBuf,
    pub(super) git_common_dir: PathBuf,
    pub storage: RepoStorage,
    pub(super) workdir: PathBuf,
    /// Canonical (absolute, resolved) version of workdir for reliable path comparisons
    /// On Windows, this uses the \\?\ UNC prefix format
    pub(super) canonical_workdir: PathBuf,
    /// Cached git author identity resolved via `git var GIT_COMMITTER_IDENT`.
    pub(super) cached_author_identity: std::sync::OnceLock<GitAuthorIdentity>,
    /// Shared across repository clones so promisor detection runs once per repository.
    pub(super) cached_has_promisor_remote: Arc<OnceLock<bool>>,
    /// `--is-bare-repository` as `find_repository` read it; `None` if found without git.
    pub(super) discovered_is_bare: Option<bool>,
}

impl Repository {
    /// Returns whether this repository has a configured promisor remote.
    ///
    /// A failed config query is treated as a promisor repository so callers
    /// avoid commands that could trigger an unbounded lazy fetch.
    pub(crate) fn has_promisor_remote(&self) -> bool {
        *self.cached_has_promisor_remote.get_or_init(|| {
            let mut args = self.global_args_for_exec();
            args.extend([
                "config".to_string(),
                "--bool".to_string(),
                "--get-regexp".to_string(),
                r"^remote\..*\.promisor$".to_string(),
            ]);

            match exec_git_allow_nonzero(&args) {
                Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|line| line.split_whitespace().last() == Some("true")),
                Ok(output) if output.status.code() == Some(1) => false,
                Ok(_) | Err(_) => true,
            }
        })
    }

    // Util for preparing global args for execution
    pub fn global_args_for_exec(&self) -> Vec<String> {
        let mut args = self.global_args.clone();
        if !args.iter().any(|arg| arg == "--no-pager") {
            args.push("--no-pager".to_string());
        }
        args
    }

    // Internal util to get the git object type for a given OID
    pub(super) fn object_type(&self, oid: &str) -> Result<String, GitAiError> {
        let reader =
            crate::operations::git::fast_reader::FastObjectReader::new(&self.git_common_dir);
        if let Some(typ) = reader.try_read_object_type(oid) {
            return Ok(typ);
        }

        let mut args = self.global_args_for_exec();
        args.push("cat-file".to_string());
        args.push("-t".to_string());
        args.push(oid.to_string());
        let output = exec_git(&args)?;
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    // Retrieve and resolve the reference pointed at by HEAD.
    // If HEAD is a symbolic ref, return the refname (e.g., "refs/heads/main").
    // Otherwise, return "HEAD".
    pub fn head<'a>(&'a self) -> Result<Reference<'a>, GitAiError> {
        use crate::operations::git::fast_reader::{FastRefReader, HeadKind};
        let reader = FastRefReader::new(&self.git_dir, &self.git_common_dir);
        match reader.try_read_head() {
            Some(HeadKind::Symbolic(refname)) => {
                return Ok(Reference {
                    repo: self,
                    ref_name: refname,
                });
            }
            Some(HeadKind::Detached(_)) => {
                return Ok(Reference {
                    repo: self,
                    ref_name: "HEAD".to_string(),
                });
            }
            None => {}
        }

        let mut args = self.global_args_for_exec();
        args.push("symbolic-ref".to_string());
        args.push("HEAD".to_string());

        let output = exec_git(&args);

        match output {
            Ok(output) if output.status.success() => {
                let refname = String::from_utf8(output.stdout)?;
                Ok(Reference {
                    repo: self,
                    ref_name: refname.trim().to_string(),
                })
            }
            _ => Ok(Reference {
                repo: self,
                ref_name: "HEAD".to_string(),
            }),
        }
    }

    // Returns the path to the .git folder for normal repositories or the repository itself for bare repositories.
    // TODO Test on bare repositories.
    pub fn path(&self) -> &Path {
        self.git_dir.as_path()
    }

    /// Returns the common git directory shared by linked worktrees.
    /// For non-worktree repositories, this is the same as `path()`.
    pub fn common_dir(&self) -> &Path {
        self.git_common_dir.as_path()
    }

    // Get the path of the working directory for this repository.
    // If this repository is bare, then None is returned.
    pub fn workdir(&self) -> Result<PathBuf, GitAiError> {
        // TODO Remove Result since this is determined at initialization now
        Ok(self.workdir.clone())
    }

    /// Canonical (symlink-resolved) working directory root.
    pub fn canonical_workdir(&self) -> &Path {
        self.canonical_workdir.as_path()
    }

    /// Returns true when this repository is bare.
    pub fn is_bare_repository(&self) -> Result<bool, GitAiError> {
        if let Some(is_bare) = self.discovered_is_bare {
            return Ok(is_bare);
        }
        let mut args = self.global_args_for_exec();
        args.push("rev-parse".to_string());
        args.push("--is-bare-repository".to_string());
        let output = exec_git(&args)?;
        let value = String::from_utf8(output.stdout)?;
        Ok(value.trim() == "true")
    }

    /// Get the canonical (absolute, resolved) path of the working directory
    /// Check if a path is within the repository's working directory.
    ///
    /// Returns `false` for paths inside nested independent git repos (subdirectories
    /// with their own `.git/` directory), since those files belong to the nested repo,
    /// not this one. Submodules (`.git` file, not directory) are transparent and still
    /// considered part of this repo.
    pub fn path_is_in_workdir(&self, path: &Path) -> bool {
        // Try canonical comparison first (most reliable, especially on Windows)
        if let Ok(canonical_path) = path.canonicalize() {
            if !canonical_path.starts_with(&self.canonical_workdir) {
                return false;
            }
            return !super::discovery_no_exec::has_intervening_git_dir(
                &canonical_path,
                &self.canonical_workdir,
            );
        }

        // Fallback for paths that don't exist yet: try to canonicalize the parent directory
        // and append the filename. This handles cases where the path contains symlinks
        // (e.g., /var -> /private/var on macOS).
        if let Some(parent) = path.parent()
            && let Some(filename) = path.file_name()
            && let Ok(canonical_parent) = parent.canonicalize()
        {
            let canonical_path = canonical_parent.join(filename);
            if !canonical_path.starts_with(&self.canonical_workdir) {
                return false;
            }
            return !super::discovery_no_exec::has_intervening_git_dir(
                &canonical_path,
                &self.canonical_workdir,
            );
        }

        // Final fallback: normalize by resolving .. and . and check against both
        // canonical and non-canonical workdir (in case of symlinks)
        let normalized = path
            .components()
            .fold(std::path::PathBuf::new(), |mut acc, component| {
                match component {
                    std::path::Component::ParentDir => {
                        acc.pop();
                    }
                    std::path::Component::CurDir => {}
                    _ => acc.push(component),
                }
                acc
            });

        // Try both canonical and non-canonical workdir to handle symlinks
        let in_canonical = normalized.starts_with(&self.canonical_workdir);
        let in_workdir = normalized.starts_with(&self.workdir);

        if !in_canonical && !in_workdir {
            return false;
        }

        // Use canonical_workdir if path matches it, otherwise use workdir
        let base = if in_canonical {
            &self.canonical_workdir
        } else {
            &self.workdir
        };

        !super::discovery_no_exec::has_intervening_git_dir(&normalized, base)
    }

    pub fn remotes(&self) -> Result<Vec<String>, GitAiError> {
        Ok(self
            .remotes_with_urls()?
            .into_iter()
            .map(|(name, _)| name)
            .collect())
    }

    // List all remotes with their URLs as tuples (name, url)
    pub fn remotes_with_urls(&self) -> Result<Vec<(String, String)>, GitAiError> {
        let config = self.get_git_config_file()?;
        Ok(super::policy::remotes_with_urls_from_config(&config))
    }

    /// Whether git-ai collection is allowed for this repository under `config`.
    ///
    /// Fetches the repository's remotes and root once and delegates to the pure
    /// `config::Config::is_allowed_repository_with_context` policy. Collection
    /// is opt-in: an empty `allowed_repositories` list denies every repository.
    pub fn is_collection_allowed(&self, config: &config::Config) -> bool {
        let remotes = self.remotes_with_urls().ok();
        let repo_root = self.canonical_workdir();
        config.is_allowed_repository_with_context(remotes.as_ref(), Some(repo_root))
    }

    pub(super) fn load_optional_config_file(
        path: &Path,
        source: gix_config::Source,
    ) -> Result<Option<gix_config::File<'static>>, GitAiError> {
        if !path.exists() {
            return Ok(None);
        }
        gix_config::File::from_path_no_includes(path.to_path_buf(), source)
            .map(Some)
            .map_err(|e| GitAiError::GixError(e.to_string()))
    }

    pub(crate) fn get_git_config_file(&self) -> Result<gix_config::File<'static>, GitAiError> {
        git_config_file_for_repo_paths(self.path(), self.common_dir())
    }

    /// Get config value for a given key as a String.
    pub fn config_get_str(&self, key: &str) -> Result<Option<String>, GitAiError> {
        self.get_git_config_file()
            .map(|cfg| cfg.string(key).map(|cow| cow.to_string()))
    }

    /// Get the effective raw Git user identity for this repository.
    ///
    /// Uses `git var GIT_COMMITTER_IDENT` which respects the full git identity precedence:
    /// `GIT_COMMITTER_NAME`/`GIT_COMMITTER_EMAIL` env vars > `user.name`/`user.email` config >
    /// system defaults.
    ///
    /// Falls back to `git config user.name` / `user.email` if `git var` fails.
    /// The result is cached per Repository instance for performance.
    ///
    /// For git-ai authorship metadata, use [`Self::effective_author_identity`] so the
    /// git-ai author config can override this raw Git identity.
    pub fn git_author_identity(&self) -> &GitAuthorIdentity {
        self.cached_author_identity
            .get_or_init(|| self.resolve_git_var_identity("GIT_COMMITTER_IDENT"))
    }

    pub fn git_author_identity_resolution(&self) -> GitIdentityResolution {
        self.resolve_git_var_identity_resolution("GIT_COMMITTER_IDENT")
    }

    /// Get the git-ai effective author identity for metadata and display.
    ///
    /// This starts from Git's effective committer identity, then overlays any
    /// configured `author.name` and/or `author.email` from git-ai config.
    pub fn effective_author_identity(&self) -> GitAuthorIdentity {
        let git_id = self.git_author_identity();
        git_id.with_author_config(&config::Config::fresh_author_cached())
    }

    /// Internal: resolve git identity via the specified `git var` variable.
    fn resolve_git_var_identity(&self, git_var: &str) -> GitAuthorIdentity {
        self.resolve_git_var_identity_resolution(git_var).identity
    }

    fn resolve_git_var_identity_resolution(&self, git_var: &str) -> GitIdentityResolution {
        resolve_git_var_identity_with_args(self.global_args_for_exec(), git_var, || {
            self.get_git_config_file()
                .ok()
                .map(|config| git_config_identity_resolution_from_config(&config).identity)
                .unwrap_or_default()
        })
    }

    /// Get the git version as a tuple (major, minor, patch).
    /// Returns None if the version cannot be parsed.
    pub fn git_version(&self) -> Option<(u32, u32, u32)> {
        probe_configured_git_version()
    }

    /// Check if the current git version supports --ignore-revs-file flag for blame.
    /// This flag was added in git 2.23.0.
    pub fn git_supports_ignore_revs_file(&self) -> bool {
        if let Some((major, minor, _)) = self.git_version() {
            // --ignore-revs-file was added in git 2.23.0
            major > 2 || (major == 2 && minor >= 23)
        } else {
            // If we can't determine the version, assume it's supported
            // to avoid breaking existing functionality
            true
        }
    }

    // Write an in-memory buffer to the ODB as a blob.
    #[allow(dead_code)]
    pub fn remote_head(&self, remote_name: &str) -> Result<String, GitAiError> {
        let mut args = self.global_args_for_exec();
        args.push("symbolic-ref".to_string());
        args.push(format!("refs/remotes/{}/HEAD", remote_name));
        args.push("--short".to_string());

        let output = exec_git(&args)?;
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    // Find a merge base between two commits
    pub fn merge_base(&self, one: String, two: String) -> Result<String, GitAiError> {
        let mut args = self.global_args_for_exec();
        args.push("merge-base".to_string());
        args.push(one.to_string());
        args.push(two.to_string());
        let output = exec_git(&args)?;
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    // Find a single object, as specified by a revision string.
    pub fn revparse_single(&self, spec: &str) -> Result<Object<'_>, GitAiError> {
        let mut args = self.global_args_for_exec();
        args.push("rev-parse".to_string());
        // args.push("-q".to_string());
        args.push("--verify".to_string());
        args.push(spec.to_string());
        let output = exec_git(&args)?;
        Ok(Object {
            repo: self,
            oid: String::from_utf8(output.stdout)?.trim().to_string(),
        })
    }

    // Non-standard method of getting a 'default' remote
    pub fn get_default_remote(&self) -> Result<Option<String>, GitAiError> {
        Ok(self.default_remote_with_url()?.map(|(name, _)| name))
    }

    /// The default remote as `(name, url)`: `origin` if it exists, otherwise
    /// the first remote. Reads the git config once.
    pub fn default_remote_with_url(&self) -> Result<Option<(String, String)>, GitAiError> {
        let mut remotes = self.remotes_with_urls()?;
        let idx = remotes
            .iter()
            .position(|(name, _)| name == "origin")
            .unwrap_or(0);
        Ok((idx < remotes.len()).then(|| remotes.swap_remove(idx)))
    }

    #[allow(dead_code)]
    pub fn push_authorship(&self, remote_name: &str) -> Result<(), GitAiError> {
        push_authorship_notes(self, remote_name)
    }

    pub fn upstream_remote(&self) -> Result<Option<String>, GitAiError> {
        // Get current branch name using exec_git
        let mut args = self.global_args_for_exec();
        args.push("branch".to_string());
        args.push("--show-current".to_string());
        let output = exec_git(&args)?;
        let branch = String::from_utf8(output.stdout)?.trim().to_string();
        if branch.is_empty() {
            return Ok(None);
        }
        let config_key = format!("branch.{}.remote", branch);
        self.config_get_str(&config_key)
    }

    // Lookup a reference to one of the commits in a repository.
    pub fn find_commit(&self, oid: String) -> Result<Commit<'_>, GitAiError> {
        let typ = self.object_type(&oid)?;
        if typ != "commit" {
            return Err(GitAiError::Generic(format!(
                "Object is not a commit: {} (type: {})",
                oid, typ
            )));
        }
        Ok(Commit { repo: self, oid })
    }

    // Lookup a reference to one of the objects in a repository.
    pub fn find_blob(&self, oid: String) -> Result<Blob<'_>, GitAiError> {
        let typ = self.object_type(&oid)?;
        if typ != "blob" {
            return Err(GitAiError::Generic(format!(
                "Object is not a blob: {} (type: {})",
                oid, typ
            )));
        }
        Ok(Blob { repo: self, oid })
    }
}
