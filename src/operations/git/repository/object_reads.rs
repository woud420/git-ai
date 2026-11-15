//! `impl Repository` methods that read file/object content in bulk: single-file
//! content at a commit and the file list for a commit.

use super::MAX_PATHSPEC_ARGS;
use super::core::Repository;
use crate::clients::git_cli::exec_git;
use crate::error::GitAiError;
use crate::operations::authorship::diff_base::empty_tree_for;
use std::collections::HashSet;

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
            args.push(empty_tree_for(commit_sha).to_string());
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
