use super::types::VirtualAttributions;
use crate::error::GitAiError;
use crate::model::working_log::CheckpointKind;
use crate::operations::authorship::attribution_tracker::{
    LineAttribution, line_attributions_to_attributions,
};
use crate::operations::commands::blame::{GitAiBlameOptions, OLDEST_AI_BLAME_DATE};
use crate::operations::git::repository::Repository;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

impl VirtualAttributions {
    /// Create a new VirtualAttributions for the given base commit with initial pathspecs
    pub async fn new_for_base_commit(
        repo: Repository,
        base_commit: String,
        pathspecs: &[String],
        blame_start_commit: Option<String>,
    ) -> Result<Self, GitAiError> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let mut virtual_attrs = VirtualAttributions {
            repo,
            base_commit,
            attributions: std::collections::HashMap::new(),
            file_contents: std::collections::HashMap::new(),
            prompts: std::collections::BTreeMap::new(),
            ts,
            blame_start_commit,
            humans: std::collections::BTreeMap::new(),
            initial_only_prompt_ids: std::collections::HashSet::new(),
            sessions: std::collections::BTreeMap::new(),
        };

        // Process all pathspecs concurrently
        if !pathspecs.is_empty() {
            virtual_attrs.add_pathspecs_concurrent(pathspecs).await?;
        }

        // After running blame, discover and load any missing prompts from blamed commits
        virtual_attrs.discover_and_load_foreign_prompts().await?;

        Ok(virtual_attrs)
    }

    /// Add a single pathspec to the virtual attributions
    #[allow(dead_code)]
    pub async fn add_pathspec(&mut self, pathspec: &str) -> Result<(), GitAiError> {
        self.add_pathspecs_concurrent(&[pathspec.to_string()]).await
    }

    /// Add multiple pathspecs concurrently
    pub(super) async fn add_pathspecs_concurrent(
        &mut self,
        pathspecs: &[String],
    ) -> Result<(), GitAiError> {
        const MAX_CONCURRENT: usize = 30;

        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT));
        let mut tasks = Vec::new();

        for pathspec in pathspecs {
            let pathspec = pathspec.clone();
            let repo = self.repo.clone();
            let base_commit = self.base_commit.clone();
            let ts = self.ts;
            let blame_start_commit = self.blame_start_commit.clone();
            let semaphore = Arc::clone(&semaphore);

            let task = async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("virtual attribution semaphore was closed");

                crate::tokio_runtime::spawn_blocking_result(move || {
                    compute_attributions_for_file(
                        &repo,
                        &base_commit,
                        &pathspec,
                        ts,
                        blame_start_commit,
                    )
                })
                .await
            };

            tasks.push(task);
        }

        // Await all tasks
        let results = futures::future::join_all(tasks).await;

        // Process results and store in HashMap
        for result in results {
            match result {
                Ok(Some((file_path, content, char_attrs, line_attrs))) => {
                    self.attributions
                        .insert(file_path.clone(), (char_attrs, line_attrs));
                    self.file_contents.insert(file_path, content);
                }
                Ok(None) => {
                    // File had no changes or couldn't be processed, skip
                }
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }
}

/// Compute attributions for a single file at a specific commit
#[allow(clippy::type_complexity)]
pub(super) fn compute_attributions_for_file(
    repo: &Repository,
    base_commit: &str,
    file_path: &str,
    ts: u128,
    blame_start_commit: Option<String>,
) -> Result<
    Option<(
        String,
        String,
        Vec<crate::operations::authorship::attribution_tracker::Attribution>,
        Vec<LineAttribution>,
    )>,
    GitAiError,
> {
    // Set up blame options
    let mut ai_blame_opts = GitAiBlameOptions::default();
    #[allow(clippy::field_reassign_with_default)]
    {
        ai_blame_opts.no_output = true;
        ai_blame_opts.return_human_authors_as_human = true;
        ai_blame_opts.use_prompt_hashes_as_names = true;
        ai_blame_opts.newest_commit = Some(base_commit.to_string());
        ai_blame_opts.oldest_commit = blame_start_commit;
        ai_blame_opts.oldest_date = Some(*OLDEST_AI_BLAME_DATE);
    }

    // Run blame at the base commit
    let ai_blame = repo.blame(file_path, &ai_blame_opts);

    match ai_blame {
        Ok((blames, _)) => {
            // Convert blame results to line attributions
            let mut line_attributions = Vec::new();
            for (line, author) in blames {
                // Skip human-only lines as they don't need tracking
                if author == CheckpointKind::Human.to_str() {
                    continue;
                }
                line_attributions.push(LineAttribution {
                    start_line: line,
                    end_line: line,
                    author_id: author.clone(),
                    overrode: None,
                });
            }

            // Get the file content at this commit to convert to character attributions
            // We need to read the file content that blame operated on
            let file_content = get_file_content_at_commit(repo, base_commit, file_path)?;

            // Convert line attributions to character attributions
            let char_attributions =
                line_attributions_to_attributions(&line_attributions, &file_content, ts);

            Ok(Some((
                file_path.to_string(),
                file_content,
                char_attributions,
                line_attributions,
            )))
        }
        Err(_) => {
            // File doesn't exist at this commit or can't be blamed, skip it
            Ok(None)
        }
    }
}

pub(super) fn get_file_content_at_commit(
    repo: &Repository,
    commit_sha: &str,
    file_path: &str,
) -> Result<String, GitAiError> {
    let commit = repo.find_commit(commit_sha.to_string())?;
    let tree = commit.tree()?;

    match tree.get_path(std::path::Path::new(file_path)) {
        Ok(entry) => {
            if let Ok(blob) = repo.find_blob(entry.id()) {
                let blob_content = blob.content().unwrap_or_default();
                Ok(String::from_utf8_lossy(&blob_content).to_string())
            } else {
                Ok(String::new())
            }
        }
        Err(_) => Ok(String::new()),
    }
}

/// Check if a file exists in a commit's tree
pub(super) fn file_exists_in_commit(
    repo: &Repository,
    commit_sha: &str,
    file_path: &str,
) -> Result<bool, GitAiError> {
    use unicode_normalization::UnicodeNormalization;

    let commit = repo.find_commit(commit_sha.to_string())?;
    let tree = commit.tree()?;
    if tree.get_path(std::path::Path::new(file_path)).is_ok() {
        return Ok(true);
    }
    // The caller's path may be NFC or NFD while the tree stores the opposite
    // form.  Try both normalisations before giving up.
    if !file_path.is_ascii() {
        let nfc_path: String = file_path.nfc().collect();
        if nfc_path != file_path && tree.get_path(std::path::Path::new(&nfc_path)).is_ok() {
            return Ok(true);
        }
        let nfd_path: String = file_path.nfd().collect();
        if nfd_path != file_path && tree.get_path(std::path::Path::new(&nfd_path)).is_ok() {
            return Ok(true);
        }
    }
    Ok(false)
}
