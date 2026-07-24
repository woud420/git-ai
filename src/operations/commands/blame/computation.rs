use std::collections::HashMap;
use std::fs;

use crate::error::GitAiError;
use crate::model::authorship_log::PromptRecord;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::authorship::line_lookup::get_line_attribution;
use crate::operations::git::notes_api::read_authorship_v3;
#[cfg(windows)]
use crate::operations::git::path_format::normalize_to_posix;
use crate::operations::git::repository::Repository;

use super::json_output::output_json_format;
use super::output::output_default_format;
use super::overlay::overlay_ai_authorship;
use super::porcelain::{output_incremental_format, output_porcelain_format};
use super::{BlameAnalysisResult, BlameHunk, GitAiBlameOptions, PreparedBlameRequest};

impl Repository {
    fn normalize_blame_file_path(&self, file_path: &str) -> Result<String, GitAiError> {
        let repo_root = self.workdir().map_err(|e| {
            GitAiError::Generic(format!("Repository has no working directory: {}", e))
        })?;

        // Normalize the file path to be relative to repo root.
        // This is important for AI authorship lookup which stores paths relative to repo root.
        let file_path_buf = std::path::Path::new(file_path);
        let relative_file_path = if file_path_buf.is_absolute() {
            // Convert absolute path to relative path.
            // Canonicalize both paths to handle symlinks (e.g., /var -> /private/var on macOS).
            let canonical_file_path = file_path_buf.canonicalize().map_err(|e| {
                GitAiError::Generic(format!(
                    "Failed to canonicalize file path '{}': {}",
                    file_path, e
                ))
            })?;
            let canonical_repo_root = repo_root.canonicalize().map_err(|e| {
                GitAiError::Generic(format!(
                    "Failed to canonicalize repository root '{}': {}",
                    repo_root.display(),
                    e
                ))
            })?;

            canonical_file_path
                .strip_prefix(&canonical_repo_root)
                .map_err(|_| {
                    GitAiError::Generic(format!(
                        "File path '{}' is not within repository root '{}'",
                        file_path,
                        repo_root.display()
                    ))
                })?
                .to_string_lossy()
                .to_string()
        } else {
            file_path.to_string()
        };

        // Normalize path separators and leading ./.
        #[cfg(windows)]
        let relative_file_path = {
            let normalized = normalize_to_posix(&relative_file_path);
            normalized
                .strip_prefix("./")
                .unwrap_or(&normalized)
                .to_string()
        };

        #[cfg(not(windows))]
        let relative_file_path = {
            relative_file_path
                .strip_prefix("./")
                .unwrap_or(&relative_file_path)
                .to_string()
        };

        Ok(relative_file_path)
    }

    fn effective_blame_options(options: &GitAiBlameOptions) -> GitAiBlameOptions {
        // For JSON output, default to HEAD to exclude uncommitted changes
        // and use prompt hashes as names so we can correlate with prompt_records.
        if options.json {
            let mut opts = options.clone();
            if opts.newest_commit.is_none() {
                opts.newest_commit = Some("HEAD".to_string());
            }
            opts.use_prompt_hashes_as_names = true;
            opts
        } else if options.show_prompt {
            let mut opts = options.clone();
            opts.use_prompt_hashes_as_names = true;
            opts
        } else {
            options.clone()
        }
    }

    fn read_blame_file_content(
        &self,
        relative_file_path: &str,
        options: &GitAiBlameOptions,
    ) -> Result<String, GitAiError> {
        // Read file content from one of:
        // 1. Provided contents_data (from --contents flag)
        // 2. A specific commit
        // 3. The working directory
        if let Some(ref data) = options.contents_data {
            // Use pre-read contents data (from --contents stdin or file)
            Ok(String::from_utf8_lossy(data).to_string())
        } else if let Some(ref commit) = options.newest_commit {
            // Read file content from the specified commit.
            // This ensures blame is independent of which branch is checked out.
            let commit_obj = self.find_commit(commit.clone())?;
            let tree = commit_obj.tree()?;

            match tree.get_path(std::path::Path::new(relative_file_path)) {
                Ok(entry) => {
                    if let Ok(blob) = self.find_blob(entry.id()) {
                        let blob_content = blob.content().unwrap_or_default();
                        Ok(String::from_utf8_lossy(&blob_content).to_string())
                    } else {
                        Err(GitAiError::Generic(format!(
                            "File '{}' is not a blob in commit {}",
                            relative_file_path, commit
                        )))
                    }
                }
                Err(_) => Err(GitAiError::Generic(format!(
                    "File '{}' not found in commit {}",
                    relative_file_path, commit
                ))),
            }
        } else {
            // Read from working directory.
            let repo_root = self.workdir().map_err(|e| {
                GitAiError::Generic(format!("Repository has no working directory: {}", e))
            })?;
            let abs_file_path = repo_root.join(relative_file_path);

            if !abs_file_path.exists() {
                return Err(GitAiError::Generic(format!(
                    "File not found: {}",
                    abs_file_path.display()
                )));
            }

            let raw_bytes = fs::read(&abs_file_path)?;
            Ok(String::from_utf8_lossy(&raw_bytes).into_owned())
        }
    }

    pub(super) fn prepare_blame_request(
        &self,
        file_path: &str,
        options: &GitAiBlameOptions,
    ) -> Result<PreparedBlameRequest, GitAiError> {
        let relative_file_path = self.normalize_blame_file_path(file_path)?;
        let options = Self::effective_blame_options(options);
        let file_content = self.read_blame_file_content(&relative_file_path, &options)?;
        let total_lines = file_content.lines().count() as u32;

        // Determine the line ranges to process.
        let line_ranges = if options.line_ranges.is_empty() {
            vec![(1, total_lines)]
        } else {
            options.line_ranges.clone()
        };

        // Validate line ranges.
        for (start, end) in &line_ranges {
            if *start == 0 || *end == 0 || start > end || *end > total_lines {
                return Err(GitAiError::Generic(format!(
                    "Invalid line range: {}:{}. File has {} lines",
                    start, end, total_lines
                )));
            }
        }

        Ok(PreparedBlameRequest {
            relative_file_path,
            file_content,
            line_ranges,
            options,
        })
    }

    #[allow(clippy::type_complexity)]
    pub(super) fn run_blame_analysis_pipeline(
        &self,
        relative_file_path: &str,
        line_ranges: &[(u32, u32)],
        options: &GitAiBlameOptions,
    ) -> Result<
        (
            BlameAnalysisResult,
            Vec<AuthorshipLog>,
            HashMap<String, Vec<String>>,
            std::collections::HashSet<String>, // commits with real authorship notes
        ),
        GitAiError,
    > {
        // Step 1: Get Git's native blame for all ranges in one invocation.
        let blame_hunks = self.blame_hunks_for_ranges(relative_file_path, line_ranges, options)?;

        // Step 2: Overlay AI authorship information.
        let (
            line_authors,
            prompt_records,
            session_records,
            humans,
            authorship_logs,
            prompt_commits,
            commits_with_notes,
        ) = overlay_ai_authorship(self, &blame_hunks, relative_file_path, options)?;

        Ok((
            BlameAnalysisResult {
                line_authors,
                prompt_records,
                session_records,
                blame_hunks,
                humans,
            },
            authorship_logs,
            prompt_commits,
            commits_with_notes,
        ))
    }

    #[allow(clippy::type_complexity)]
    pub fn blame(
        &self,
        file_path: &str,
        options: &GitAiBlameOptions,
    ) -> Result<(HashMap<u32, String>, HashMap<String, PromptRecord>), GitAiError> {
        let request = self.prepare_blame_request(file_path, options)?;
        let lines: Vec<&str> = request.file_content.lines().collect();
        let (analysis, authorship_logs, prompt_commits, commits_with_notes) = self
            .run_blame_analysis_pipeline(
                &request.relative_file_path,
                &request.line_ranges,
                &request.options,
            )?;
        let BlameAnalysisResult {
            line_authors,
            prompt_records,
            session_records: _,
            blame_hunks: _,
            humans: _,
        } = analysis;

        if request.options.no_output {
            return Ok((line_authors, prompt_records));
        }

        // Output based on format
        if options.json {
            output_json_format(
                self,
                &line_authors,
                &prompt_records,
                &authorship_logs,
                &prompt_commits,
                &request.relative_file_path,
            )?;
        } else if request.options.porcelain || request.options.line_porcelain {
            output_porcelain_format(
                self,
                &line_authors,
                &request.relative_file_path,
                &lines,
                &request.line_ranges,
                &request.options,
                &commits_with_notes,
            )?;
        } else if request.options.incremental {
            output_incremental_format(
                self,
                &line_authors,
                &request.relative_file_path,
                &lines,
                &request.line_ranges,
                &request.options,
                &commits_with_notes,
            )?;
        } else {
            output_default_format(
                self,
                &line_authors,
                &prompt_records,
                &request.relative_file_path,
                &lines,
                &request.line_ranges,
                &request.options,
            )?;
        }

        Ok((line_authors, prompt_records))
    }

    pub fn blame_analysis(
        &self,
        file_path: &str,
        options: &GitAiBlameOptions,
    ) -> Result<BlameAnalysisResult, GitAiError> {
        let request = self.prepare_blame_request(file_path, options)?;
        let (analysis, _authorship_logs, _prompt_commits, _commits_with_notes) = self
            .run_blame_analysis_pipeline(
                &request.relative_file_path,
                &request.line_ranges,
                &request.options,
            )?;
        Ok(analysis)
    }

    /// Post-process blame hunks to populate ai_human_author from authorship logs.
    /// For each hunk, looks up the authorship log for its commit and finds the human_author
    /// from the prompt record that covers lines in the hunk.
    /// If `split_hunks_by_ai_author` is true and different lines in a hunk have different
    /// human_authors, the hunk is split into multiple hunks.
    pub(super) fn populate_ai_human_authors(
        &self,
        hunks: Vec<BlameHunk>,
        file_path: &str,
        options: &GitAiBlameOptions,
    ) -> Result<Vec<BlameHunk>, GitAiError> {
        // Cache authorship logs by commit SHA to avoid repeated lookups
        let mut commit_authorship_cache: HashMap<String, Option<_>> = HashMap::new();
        // Cache for foreign prompts to avoid repeated grepping
        let mut foreign_prompts_cache: HashMap<String, Option<PromptRecord>> = HashMap::new();

        let mut result_hunks: Vec<BlameHunk> = Vec::new();

        for hunk in hunks {
            // Get or fetch the authorship log for this commit
            let authorship_log = if let Some(cached) = commit_authorship_cache.get(&hunk.commit_sha)
            {
                cached.clone()
            } else {
                let authorship = read_authorship_v3(self, &hunk.commit_sha).ok();
                commit_authorship_cache.insert(hunk.commit_sha.clone(), authorship.clone());
                authorship
            };

            // If we have an authorship log, look up human_author for each line
            if let Some(ref authorship_log) = authorship_log {
                // Collect human_author for each line in this hunk
                let num_lines = hunk.range.1 - hunk.range.0 + 1;
                let mut line_authors: Vec<Option<String>> = Vec::with_capacity(num_lines as usize);

                for i in 0..num_lines {
                    let orig_line_num = hunk.orig_range.0 + i;
                    let human_author = get_line_attribution(
                        authorship_log,
                        self,
                        file_path,
                        orig_line_num,
                        &mut foreign_prompts_cache,
                    )
                    .and_then(|(_author, _prompt_hash, prompt)| prompt)
                    .and_then(|prompt_record| prompt_record.human_author);
                    line_authors.push(human_author);
                }

                if options.split_hunks_by_ai_author {
                    // Split hunk by consecutive lines with the same human_author
                    let mut current_start_idx: u32 = 0;
                    let mut current_author = line_authors.first().cloned().flatten();

                    for (i, author) in line_authors.iter().enumerate() {
                        let author_flat = author.clone();
                        if author_flat != current_author {
                            // Create a hunk for the previous group
                            let group_start = hunk.range.0 + current_start_idx;
                            let group_end = hunk.range.0 + (i as u32) - 1;
                            let orig_group_start = hunk.orig_range.0 + current_start_idx;
                            let orig_group_end = hunk.orig_range.0 + (i as u32) - 1;

                            let mut new_hunk = hunk.clone();
                            new_hunk.range = (group_start, group_end);
                            new_hunk.orig_range = (orig_group_start, orig_group_end);
                            new_hunk.ai_human_author = current_author.clone();
                            result_hunks.push(new_hunk);

                            // Start a new group
                            current_start_idx = i as u32;
                            current_author = author_flat;
                        }
                    }

                    // Don't forget the last group
                    let group_start = hunk.range.0 + current_start_idx;
                    let group_end = hunk.range.1;
                    let orig_group_start = hunk.orig_range.0 + current_start_idx;
                    let orig_group_end = hunk.orig_range.1;

                    let mut new_hunk = hunk.clone();
                    new_hunk.range = (group_start, group_end);
                    new_hunk.orig_range = (orig_group_start, orig_group_end);
                    new_hunk.ai_human_author = current_author;
                    result_hunks.push(new_hunk);
                } else {
                    // Don't split - just use the first human_author found
                    let mut new_hunk = hunk;
                    new_hunk.ai_human_author = line_authors.into_iter().flatten().next();
                    result_hunks.push(new_hunk);
                }
            } else {
                // No authorship log, keep hunk as-is
                result_hunks.push(hunk);
            }
        }

        Ok(result_hunks)
    }
}
