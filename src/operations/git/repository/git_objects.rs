//! Typed wrappers around git tree entries, trees, blobs, and references, plus
//! the [`References`] iterator. Commit-oriented wrappers ([`Object`],
//! [`CommitRange`], [`Commit`], [`Parents`]) live in the sibling `commits`
//! module.

use super::commits::Commit;
use super::core::Repository;
use crate::clients::git_cli::exec_git;
use crate::error::GitAiError;
use crate::operations::git::oid::is_full_oid;
use std::fs;
use std::path::Path;

pub struct TreeEntry<'a> {
    _repo: std::marker::PhantomData<&'a Repository>,
    // Object id (SHA-1/oid) that this tree entry points to
    oid: String,
}

impl<'a> TreeEntry<'a> {
    // Get the id of the object pointed by the entry
    pub fn id(&self) -> String {
        self.oid.clone()
    }
}

pub struct Tree<'a> {
    pub(super) repo: &'a Repository,
    pub(super) oid: String,
}

/// Construct a [`Tree`] from within the `repository` module tree. Used by
/// sibling modules that resolve tree OIDs but cannot name `Tree`'s private
/// fields across module boundaries directly.
pub(super) fn make_tree<'a>(repo: &'a Repository, oid: String) -> Tree<'a> {
    Tree { repo, oid }
}

impl<'a> Tree<'a> {
    // Get the id of the tree
    pub fn id(&self) -> String {
        self.oid.clone()
    }

    #[allow(dead_code)]
    #[allow(clippy::should_implement_trait)]
    pub fn clone(&self) -> Tree<'a> {
        Tree {
            repo: self.repo,
            oid: self.oid.clone(),
        }
    }

    // Retrieve a tree entry contained in a tree or in any of its subtrees, given its relative path.
    pub fn get_path(&self, path: &Path) -> Result<TreeEntry<'a>, GitAiError> {
        let reader =
            crate::operations::git::fast_reader::FastObjectReader::new(&self.repo.git_common_dir);
        if let Some(blob_oid) = reader.try_tree_entry_for_path(&self.oid, path) {
            return Ok(TreeEntry {
                _repo: std::marker::PhantomData,
                oid: blob_oid,
            });
        }

        let mut args = self.repo.global_args_for_exec();
        args.push("ls-tree".to_string());
        args.push("-z".to_string());
        args.push("-r".to_string());
        args.push(self.oid.clone());
        args.push("--".to_string());
        let path_str = path.to_string_lossy().to_string();
        args.push(path_str.clone());

        let output = exec_git(&args)?;
        let bytes = output.stdout;

        // Each record: "<mode> <type> <object>\t<file>\0"
        // We expect at most one record for an exact path query.
        let mut found_entry: Option<TreeEntry<'a>> = None;

        for chunk in bytes.split(|b| *b == 0u8) {
            if chunk.is_empty() {
                continue;
            }
            // Split metadata and path on first tab
            let mut parts = chunk.splitn(2, |b| *b == b'\t');
            let meta = parts.next().unwrap_or(&[]);
            let file_bytes = parts.next().unwrap_or(&[]);

            // Parse meta: "<mode> <type> <object>"
            let meta_str = String::from_utf8_lossy(meta);
            let mut meta_iter = meta_str.split_whitespace();
            let mode = meta_iter.next().unwrap_or("").to_string();
            let object_type = meta_iter.next().unwrap_or("").to_string();
            let oid = meta_iter.next().unwrap_or("").to_string();

            if mode.is_empty() || object_type.is_empty() || oid.is_empty() {
                continue;
            }

            let file_path = String::from_utf8_lossy(file_bytes).to_string();

            // Prefer exact path match if multiple records somehow appear
            if found_entry.is_none() || file_path == path_str {
                found_entry = Some(TreeEntry {
                    _repo: std::marker::PhantomData,
                    oid,
                });
            }
        }

        match found_entry {
            Some(entry) => Ok(entry),
            None => Err(GitAiError::Generic(format!(
                "Path not found in tree: {}",
                path.to_string_lossy()
            ))),
        }
    }
}

pub struct Blob<'a> {
    pub(super) repo: &'a Repository,
    pub(super) oid: String,
}

impl<'a> Blob<'a> {
    #[allow(dead_code)]
    pub fn id(&self) -> String {
        self.oid.clone()
    }

    pub fn content(&self) -> Result<Vec<u8>, GitAiError> {
        let reader =
            crate::operations::git::fast_reader::FastObjectReader::new(&self.repo.git_common_dir);
        if let Some(data) = reader.try_read_blob(&self.oid) {
            return Ok(data);
        }

        let mut args = self.repo.global_args_for_exec();
        args.push("cat-file".to_string());
        args.push("blob".to_string());
        args.push(self.oid.clone());
        let output = exec_git(&args)?;
        Ok(output.stdout)
    }
}

pub struct Reference<'a> {
    pub(super) repo: &'a Repository,
    pub(super) ref_name: String,
}

fn shorthand_ref_name(ref_name: &str) -> Option<&str> {
    if ref_name == "HEAD" {
        Some("HEAD")
    } else if let Some(name) = ref_name.strip_prefix("refs/heads/") {
        Some(name)
    } else {
        ref_name.strip_prefix("refs/remotes/")
    }
}

fn has_resolved_loose_ref(git_common_dir: &Path, git_dir: &Path, ref_name: &str) -> bool {
    [git_common_dir, git_dir].into_iter().any(|base| {
        fs::read_to_string(base.join(ref_name))
            .ok()
            .is_some_and(|contents| is_full_oid(contents.trim()))
    })
}

impl<'a> Reference<'a> {
    pub fn name(&self) -> Option<&str> {
        Some(&self.ref_name)
    }

    pub fn shorthand(&self) -> Result<String, GitAiError> {
        if let Some(name) = shorthand_ref_name(&self.ref_name) {
            if self.ref_name == "HEAD" {
                return Ok(name.to_string());
            }

            if has_resolved_loose_ref(
                &self.repo.git_common_dir,
                &self.repo.git_dir,
                &self.ref_name,
            ) {
                return Ok(name.to_string());
            }
        }

        let mut args = self.repo.global_args_for_exec();
        args.push("rev-parse".to_string());
        args.push("--abbrev-ref".to_string());
        args.push(self.ref_name.clone());
        let output = exec_git(&args)?;
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    pub fn target(&self) -> Result<String, GitAiError> {
        use crate::operations::git::fast_reader::{FastRefReader, HeadKind};
        let reader = FastRefReader::new(&self.repo.git_dir, &self.repo.git_common_dir);
        if self.ref_name == "HEAD" {
            match reader.try_read_head() {
                Some(HeadKind::Detached(oid)) => return Ok(oid),
                Some(HeadKind::Symbolic(refname)) => {
                    if let Some(sha) = reader.try_resolve_ref(&refname) {
                        return Ok(sha);
                    }
                }
                None => {}
            }
        } else if let Some(sha) = reader.try_resolve_ref(&self.ref_name) {
            return Ok(sha);
        }

        let mut args = self.repo.global_args_for_exec();
        args.push("rev-parse".to_string());
        args.push(self.ref_name.clone());
        let output = exec_git(&args)?;
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    // Peel a reference to a commit This method recursively peels the reference until it reaches a commit.
    #[allow(dead_code)]
    pub fn peel_to_commit(&self) -> Result<Commit<'a>, GitAiError> {
        let mut args = self.repo.global_args_for_exec();
        args.push("rev-parse".to_string());
        // args.push("-q".to_string());
        args.push("--verify".to_string());
        args.push(format!("{}^{}", self.ref_name, "{commit}"));
        let output = exec_git(&args)?;
        Ok(super::commits::make_commit(
            self.repo,
            String::from_utf8(output.stdout)?.trim().to_string(),
        ))
    }
}

pub struct References<'a> {
    repo: &'a Repository,
    refs: Vec<String>,
    index: usize,
}

impl<'a> Iterator for References<'a> {
    type Item = Result<Reference<'a>, GitAiError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.refs.len() {
            return None;
        }
        let ref_name = self.refs[self.index].clone();
        self.index += 1;
        Some(Ok(Reference {
            repo: self.repo,
            ref_name,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{has_resolved_loose_ref, shorthand_ref_name};
    use std::fs;

    #[test]
    fn shorthand_ref_name_handles_head_and_standard_symbolic_refs() {
        assert_eq!(shorthand_ref_name("HEAD"), Some("HEAD"));
        assert_eq!(shorthand_ref_name("refs/heads/feature"), Some("feature"));
        assert_eq!(
            shorthand_ref_name("refs/remotes/origin/main"),
            Some("origin/main")
        );
        assert_eq!(shorthand_ref_name("refs/tags/v1.0.0"), None);
    }

    #[test]
    fn loose_ref_check_does_not_accept_a_packed_only_ref() {
        let git_dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(git_dir.path().join("refs/heads")).unwrap();
        fs::write(
            git_dir.path().join("packed-refs"),
            "0123456789abcdef0123456789abcdef01234567 refs/heads/main\n",
        )
        .unwrap();

        assert!(!has_resolved_loose_ref(
            git_dir.path(),
            git_dir.path(),
            "refs/heads/main"
        ));
        fs::write(
            git_dir.path().join("refs/heads/main"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .unwrap();
        assert!(has_resolved_loose_ref(
            git_dir.path(),
            git_dir.path(),
            "refs/heads/main"
        ));
    }
}
