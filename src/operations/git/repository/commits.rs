//! Commit-oriented git-object wrappers: a peelable [`Object`], a [`CommitRange`]
//! with its iterator, [`Commit`], and the [`Parents`] iterator. Sibling
//! wrappers for trees, blobs, and references live in `git_objects`.

use super::core::Repository;
use super::git_objects::Tree;
use crate::clients::git_cli::exec_git;
use crate::error::GitAiError;

pub struct Object<'a> {
    pub(super) repo: &'a Repository,
    pub(super) oid: String,
}

impl<'a> Object<'a> {
    pub fn id(&self) -> String {
        self.oid.clone()
    }

    // Recursively peel an object until a commit is found.
    pub fn peel_to_commit(&self) -> Result<Commit<'a>, GitAiError> {
        let mut args = self.repo.global_args_for_exec();
        args.push("rev-parse".to_string());
        // args.push("-q".to_string());
        args.push("--verify".to_string());
        args.push(format!("{}^{}", self.oid, "{commit}"));
        let output = exec_git(&args)?;
        Ok(Commit {
            repo: self.repo,
            oid: String::from_utf8(output.stdout)?.trim().to_string(),
        })
    }
}

#[derive(Debug, Clone)]

pub struct CommitRange<'a> {
    repo: &'a Repository,
    pub start_oid: String,
    pub end_oid: String,
    pub refname: String,
}

impl<'a> CommitRange<'a> {
    /// Create a new CommitRange with automatic refname inference.
    /// If refname is None, tries to find a single ref pointing to end_oid.
    /// If exactly one ref is found, uses that. Otherwise falls back to current HEAD.
    pub fn new_infer_refname(
        repo: &'a Repository,
        start_oid: String,
        end_oid: String,
        refname: Option<String>,
    ) -> Result<Self, GitAiError> {
        // Resolve start_oid and end_oid to actual commit SHAs
        let resolved_start = repo.revparse_single(&start_oid)?.id();
        let resolved_end = repo.revparse_single(&end_oid)?.id();

        let inferred_refname = match refname {
            Some(name) => name,
            None => {
                // Try to find refs pointing to resolved end_oid
                let mut args = repo.global_args_for_exec();
                args.push("for-each-ref".to_string());
                args.push("--points-at".to_string());
                args.push(resolved_end.clone());
                args.push("--format=%(refname)".to_string());

                let refs = match exec_git(&args) {
                    Ok(output) => {
                        let stdout = String::from_utf8(output.stdout).unwrap_or_default();
                        let refs: Vec<String> = stdout
                            .lines()
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        refs
                    }
                    Err(_) => Vec::new(),
                };

                // If exactly one ref found, use it
                if refs.len() == 1 {
                    refs[0].clone()
                } else {
                    // Fall back to current HEAD
                    match repo.head() {
                        Ok(head_ref) => head_ref.name().unwrap_or("HEAD").to_string(),
                        Err(_) => "HEAD".to_string(),
                    }
                }
            }
        };

        Ok(Self {
            repo,
            start_oid: resolved_start,
            end_oid: resolved_end,
            refname: inferred_refname,
        })
    }

    pub fn repo(&self) -> &'a Repository {
        self.repo
    }

    pub fn is_valid(&self) -> Result<(), GitAiError> {
        const EMPTY_TREE_HASH: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

        // Check that both commits exist
        // Skip validation for empty tree hash - it's a special git object that may not exist in the repo
        if self.start_oid != EMPTY_TREE_HASH {
            self.repo.find_commit(self.start_oid.clone())?;
        }
        self.repo.find_commit(self.end_oid.clone())?;

        // Check that both commits exist on the refname
        // Use git merge-base --is-ancestor <commit> <refname>
        // Skip merge-base check for empty tree hash since it's not part of commit history
        if self.start_oid != EMPTY_TREE_HASH {
            let mut args = self.repo.global_args_for_exec();
            args.push("merge-base".to_string());
            args.push("--is-ancestor".to_string());
            args.push(self.start_oid.clone());
            args.push(self.refname.clone());

            exec_git(&args).map_err(|_| {
                GitAiError::Generic(format!(
                    "Commit {} is not reachable from refname {}",
                    self.start_oid, self.refname
                ))
            })?;
        }

        let mut args = self.repo.global_args_for_exec();
        args.push("merge-base".to_string());
        args.push("--is-ancestor".to_string());
        args.push(self.end_oid.clone());
        args.push(self.refname.clone());

        exec_git(&args).map_err(|_| {
            GitAiError::Generic(format!(
                "Commit {} is not reachable from refname {}",
                self.end_oid, self.refname
            ))
        })?;

        // Check that start is an ancestor of end (direct path between them)
        // Skip for empty tree hash - it's not part of the commit DAG
        if self.start_oid != EMPTY_TREE_HASH {
            let mut args = self.repo.global_args_for_exec();
            args.push("merge-base".to_string());
            args.push("--is-ancestor".to_string());
            args.push(self.start_oid.clone());
            args.push(self.end_oid.clone());

            exec_git(&args).map_err(|_| {
                GitAiError::Generic(format!(
                    "Commit {} is not an ancestor of {}",
                    self.start_oid, self.end_oid
                ))
            })?;
        }

        Ok(())
    }

    pub fn all_commits(&self) -> Vec<String> {
        let mut commits = Vec::new();
        let itt = self.clone().into_iter();

        for commit in itt {
            commits.push(commit.id());
        }
        commits
    }
}

impl<'a> IntoIterator for CommitRange<'a> {
    type Item = Commit<'a>;
    type IntoIter = CommitRangeIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        // Empty range - return empty iterator
        if self.start_oid.is_empty() && self.end_oid.is_empty() {
            return CommitRangeIterator {
                repo: self.repo,
                commit_oids: Vec::new(),
                index: 0,
            };
        }

        // ie for single commit branches
        if self.start_oid == self.end_oid {
            return CommitRangeIterator {
                repo: self.repo,
                commit_oids: vec![self.end_oid.clone()],
                index: 0,
            };
        }

        // Use git rev-list to get all commits between start and end
        // Format: start_oid..end_oid means commits reachable from end_oid but not from start_oid
        let mut args = self.repo.global_args_for_exec();
        args.push("rev-list".to_string());
        args.push(format!("{}..{}", self.start_oid, self.end_oid));

        let commit_oids: Vec<String> = match exec_git(&args) {
            Ok(output) => {
                let stdout = String::from_utf8(output.stdout).unwrap_or_default();
                stdout
                    .lines()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            }
            Err(_) => Vec::new(), // If they don't share lineage or error occurs, return empty
        };

        CommitRangeIterator {
            repo: self.repo,
            commit_oids,
            index: 0,
        }
    }
}

pub struct CommitRangeIterator<'a> {
    repo: &'a Repository,
    commit_oids: Vec<String>,
    index: usize,
}

impl<'a> Iterator for CommitRangeIterator<'a> {
    type Item = Commit<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.commit_oids.len() {
            return None;
        }
        let oid = self.commit_oids[self.index].clone();
        self.index += 1;
        Some(Commit {
            repo: self.repo,
            oid,
        })
    }
}

pub struct Commit<'a> {
    pub(super) repo: &'a Repository,
    pub(super) oid: String,
}

/// Construct a [`Commit`] from within the `repository` module tree. Used by
/// sibling modules (e.g. `Reference::peel_to_commit`) that resolve commit OIDs
/// but cannot name `Commit`'s private fields across module boundaries directly.
pub(super) fn make_commit<'a>(repo: &'a Repository, oid: String) -> Commit<'a> {
    Commit { repo, oid }
}

impl<'a> Commit<'a> {
    pub fn id(&self) -> String {
        self.oid.clone()
    }

    pub fn tree(&self) -> Result<Tree<'a>, GitAiError> {
        let reader =
            crate::operations::git::fast_reader::FastObjectReader::new(&self.repo.git_common_dir);
        if let Some(tree_oid) = reader.try_read_commit_tree_oid(&self.oid) {
            return Ok(super::git_objects::make_tree(self.repo, tree_oid));
        }

        let mut args = self.repo.global_args_for_exec();
        args.push("rev-parse".to_string());
        args.push("--verify".to_string());
        args.push(format!("{}^{}", self.oid, "{tree}"));
        let output = exec_git(&args)?;
        Ok(super::git_objects::make_tree(
            self.repo,
            String::from_utf8(output.stdout)?.trim().to_string(),
        ))
    }

    pub fn parent(&self, i: usize) -> Result<Commit<'a>, GitAiError> {
        let mut args = self.repo.global_args_for_exec();
        args.push("rev-parse".to_string());
        // args.push("-q".to_string());
        args.push("--verify".to_string());
        // libgit2 uses 0-based indexing; Git's rev syntax uses 1-based parent selectors.
        args.push(format!("{}^{}", self.oid, i + 1));
        let output = exec_git(&args)?;
        Ok(Commit {
            repo: self.repo,
            oid: String::from_utf8(output.stdout)?.trim().to_string(),
        })
    }

    // Return an iterator over the parents of this commit.
    pub fn parents(&self) -> Parents<'a> {
        // Use `git show -s --format=%P <oid>` to get whitespace-separated parent OIDs
        let mut args = self.repo.global_args_for_exec();
        args.push("show".to_string());
        args.push("-s".to_string());
        args.push("--format=%P".to_string());
        args.push(self.oid.clone());

        let parent_oids: Vec<String> = match exec_git(&args) {
            Ok(output) => {
                let stdout = String::from_utf8(output.stdout).unwrap_or_default();
                stdout.split_whitespace().map(|s| s.to_string()).collect()
            }
            Err(_) => Vec::new(),
        };

        Parents {
            repo: self.repo,
            parent_oids,
            index: 0,
        }
    }

    // Get the number of parents of this commit.
    // Use the parents iterator to return an iterator over all parents.
    #[allow(dead_code)]
    pub fn parent_count(&self) -> Result<usize, GitAiError> {
        Ok(self.parents().count())
    }

    // Get the short "summary" of the git commit message. The returned message is the summary of the commit, comprising the first paragraph of the message with whitespace trimmed and squashed. None may be returned if an error occurs or if the summary is not valid utf-8.
    pub fn summary(&self) -> Result<String, GitAiError> {
        let mut args = self.repo.global_args_for_exec();
        args.push("show".to_string());
        args.push("-s".to_string());
        args.push("--no-notes".to_string());
        args.push("--encoding=UTF-8".to_string());
        args.push("--format=%s".to_string());
        args.push(self.oid.clone());
        let output = exec_git(&args)?;
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    // Get the body of the git commit message (everything after the first paragraph).
    // Returns an empty string if there is no body.
    pub fn body(&self) -> Result<String, GitAiError> {
        let mut args = self.repo.global_args_for_exec();
        args.push("show".to_string());
        args.push("-s".to_string());
        args.push("--no-notes".to_string());
        args.push("--encoding=UTF-8".to_string());
        args.push("--format=%b".to_string());
        args.push(self.oid.clone());
        let output = exec_git(&args)?;
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    /// Find the first parent that exists on the specified refname
    ///
    /// This is useful for merge commits where we want to find the parent on a specific branch
    /// (e.g., main) rather than just taking the first parent, which might not be correct in
    /// complex merge histories with back-and-forth merges.
    ///
    /// # Arguments
    /// * `refname` - The reference name to search for (e.g., "main", "refs/heads/main")
    ///
    /// # Returns
    /// The first parent commit that is reachable from the specified refname
    pub fn parent_on_refname(&self, refname: &str) -> Result<Commit<'a>, GitAiError> {
        // Normalize the refname to fully qualified form
        let fq_refname = {
            let mut rp_args = self.repo.global_args_for_exec();
            rp_args.push("rev-parse".to_string());
            rp_args.push("--verify".to_string());
            rp_args.push("--symbolic-full-name".to_string());
            rp_args.push(refname.to_string());

            match exec_git(&rp_args) {
                Ok(output) => {
                    let s = String::from_utf8(output.stdout).unwrap_or_default();
                    let s = s.trim();
                    if s.is_empty() {
                        if refname.starts_with("refs/") {
                            refname.to_string()
                        } else {
                            format!("refs/heads/{}", refname)
                        }
                    } else {
                        s.to_string()
                    }
                }
                Err(_) => {
                    if refname.starts_with("refs/") {
                        refname.to_string()
                    } else {
                        format!("refs/heads/{}", refname)
                    }
                }
            }
        };

        // Iterate through parents and find the first one that's on the refname
        for parent in self.parents() {
            let parent_sha = parent.id();

            // Check if this parent is an ancestor of the refname
            // git merge-base --is-ancestor <parent> <refname>
            let mut args = self.repo.global_args_for_exec();
            args.push("merge-base".to_string());
            args.push("--is-ancestor".to_string());
            args.push(parent_sha.clone());
            args.push(fq_refname.clone());

            if exec_git(&args).is_ok() {
                return Ok(parent);
            }
        }

        // If no parent is on the refname, return an error
        Err(GitAiError::Generic(format!(
            "No parent of commit {} is reachable from refname {}",
            self.oid, refname
        )))
    }
}

pub struct Parents<'a> {
    repo: &'a Repository,
    parent_oids: Vec<String>,
    index: usize,
}

impl<'a> Iterator for Parents<'a> {
    type Item = Commit<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.parent_oids.len() {
            return None;
        }
        let oid = self.parent_oids[self.index].clone();
        self.index += 1;
        Some(Commit {
            repo: self.repo,
            oid,
        })
    }
}
