//! `impl Repository` methods that read file/object content in bulk: single-file
//! content at a commit, all staged files' content and blob OIDs, and the file
//! list for a commit.

use super::core::Repository;
use super::discovery_no_exec::repository_object_hash_kind_for_path_no_git_exec;
use crate::clients::git_cli::exec_git;
use crate::error::GitAiError;
use crate::operations::git::status::MAX_PATHSPEC_ARGS;
use gix_index::entry::Stage;
use std::collections::{HashMap, HashSet};

impl Repository {
    /// Get the content of a file at a specific commit
    /// Uses `git show <commit>:<path>` for efficient single-call retrieval
    #[allow(dead_code)]
    pub fn get_file_content(
        &self,
        file_path: &str,
        commit_hash: &str,
    ) -> Result<Vec<u8>, GitAiError> {
        let mut args = self.global_args_for_exec();
        args.push("show".to_string());
        args.push(format!("{}:{}", commit_hash, file_path));
        let output = exec_git(&args)?;
        Ok(output.stdout)
    }

    /// Get content of all staged files concurrently
    /// Returns a HashMap of file paths to their staged content as strings
    /// Skips files that fail to read or aren't valid UTF-8
    pub fn get_all_staged_files_content(
        &self,
        file_paths: &[String],
    ) -> Result<HashMap<String, String>, GitAiError> {
        use futures::future::join_all;
        use std::sync::Arc;

        const MAX_CONCURRENT: usize = 30;

        let repo_global_args = self.global_args_for_exec();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT));

        let futures: Vec<_> = file_paths
            .iter()
            .map(|file_path| {
                let mut args = repo_global_args.clone();
                args.push("show".to_string());
                args.push(format!(":{}", file_path));
                let file_path = file_path.clone();
                let semaphore = semaphore.clone();

                async move {
                    let _permit = semaphore
                        .acquire_owned()
                        .await
                        .expect("staged file semaphore was closed");
                    let result = crate::tokio_runtime::spawn_blocking_result(move || {
                        exec_git(&args).and_then(|output| {
                            String::from_utf8(output.stdout)
                                .map_err(|e| GitAiError::Utf8Error(e.utf8_error()))
                        })
                    })
                    .await;
                    (file_path, result)
                }
            })
            .collect();

        let results = crate::tokio_runtime::block_on(async { join_all(futures).await });

        let mut staged_files = HashMap::new();
        for (file_path, result) in results {
            if let Ok(content) = result {
                staged_files.insert(file_path, content);
            }
        }

        Ok(staged_files)
    }

    /// Get blob OIDs for all stage-0 entries currently present in the index.
    pub fn get_all_staged_file_blob_oids(&self) -> Result<HashMap<String, String>, GitAiError> {
        let mut staged_blobs = HashMap::new();
        let object_hash = repository_object_hash_kind_for_path_no_git_exec(self.path())?;
        let index_path = self.path().join("index");
        let index = gix_index::File::at(index_path, object_hash, true, Default::default())
            .map_err(|err| GitAiError::GixError(err.to_string()))?;

        for entry in index.entries() {
            if entry.stage() != Stage::Unconflicted {
                continue;
            }
            let file_path = entry.path(&index).to_string();
            if !file_path.trim().is_empty() {
                staged_blobs.insert(file_path, entry.id.to_string());
            }
        }

        Ok(staged_blobs)
    }

    /// List all files changed in a commit
    /// Returns a HashSet of file paths relative to the repository root
    pub fn list_commit_files(
        &self,
        commit_sha: &str,
        pathspecs: Option<&HashSet<String>>,
    ) -> Result<HashSet<String>, GitAiError> {
        let mut args = self.global_args_for_exec();
        args.push("diff-tree".to_string());
        args.push("--no-commit-id".to_string());
        args.push("--name-only".to_string());
        args.push("-r".to_string());
        args.push("-z".to_string()); // NUL-separated output for proper UTF-8 handling

        // Find the commit to check if it has a parent
        let commit = self.find_commit(commit_sha.to_string())?;

        // For initial commits (no parent), compare against the empty tree
        if commit.parent_count()? == 0 {
            let empty_tree = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
            args.push(empty_tree.to_string());
        }

        args.push(commit_sha.to_string());

        // Add pathspecs if provided (only as CLI args when under threshold)
        let needs_post_filter = if let Some(paths) = pathspecs {
            // for case where pathspec filter provided BUT not pathspecs.
            // otherwise it would default to full repo
            if paths.is_empty() {
                return Ok(HashSet::new());
            }
            if paths.len() > MAX_PATHSPEC_ARGS {
                true
            } else {
                args.push("--".to_string());
                for path in paths {
                    args.push(path.clone());
                }
                false
            }
        } else {
            false
        };

        let output = exec_git(&args)?;

        // With -z, output is NUL-separated. The output may contain a trailing NUL.
        let mut files: HashSet<String> = output
            .stdout
            .split(|&b| b == 0)
            .filter(|bytes| !bytes.is_empty())
            .filter_map(|bytes| String::from_utf8(bytes.to_vec()).ok())
            .collect();

        if needs_post_filter && let Some(paths) = pathspecs {
            files.retain(|path| paths.contains(path));
        }

        Ok(files)
    }
}
