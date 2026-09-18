use std::collections::HashMap;

use crate::error::GitAiError;
use crate::model::authorship_log::PromptRecord;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::authorship::line_lookup::get_line_attribution;
use crate::operations::git::notes_api::read_authorship_v3_batch;
use crate::operations::git::repository::Repository;

use super::{BlameHunk, GitAiBlameOptions};

pub(super) fn read_hunk_authorship(
    repo: &Repository,
    hunks: &[BlameHunk],
) -> Result<HashMap<String, AuthorshipLog>, GitAiError> {
    let commits = hunks
        .iter()
        .map(|hunk| hunk.commit_sha.clone())
        .collect::<Vec<_>>();
    read_authorship_v3_batch(repo, &commits)
}

impl Repository {
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
        let commit_authorship_cache = read_hunk_authorship(self, &hunks)?;
        // Cache for foreign prompts to avoid repeated grepping
        let mut foreign_prompts_cache: HashMap<String, Option<PromptRecord>> = HashMap::new();

        let mut result_hunks: Vec<BlameHunk> = Vec::new();

        for hunk in hunks {
            let authorship_log = commit_authorship_cache.get(&hunk.commit_sha);

            // If we have an authorship log, look up human_author for each line
            if let Some(authorship_log) = authorship_log {
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
