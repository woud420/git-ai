use super::types::VirtualAttributions;
use crate::error::GitAiError;
use crate::model::authorship_log::{PromptRecord, SessionRecord};
use crate::operations::git::repository::Repository;
use std::sync::Arc;

impl VirtualAttributions {
    /// Discover and load prompts/sessions from blamed commits that aren't in our maps
    pub(super) async fn discover_and_load_foreign_prompts(&mut self) -> Result<(), GitAiError> {
        use std::collections::HashSet;

        // Collect all unique author_ids from attributions
        let mut all_author_ids: HashSet<String> = HashSet::new();
        for (char_attrs, _line_attrs) in self.attributions.values() {
            for attr in char_attrs {
                all_author_ids.insert(attr.author_id.clone());
            }
        }

        // Separate session IDs from prompt/human IDs
        let mut missing_session_ids: HashSet<String> = HashSet::new();
        let mut missing_prompt_ids: Vec<String> = Vec::new();

        for id in all_author_ids {
            if id.starts_with("s_") {
                let session_key = id.split("::").next().unwrap_or(&id).to_string();
                if !self.sessions.contains_key(&session_key) {
                    missing_session_ids.insert(session_key);
                }
            } else if !self.prompts.contains_key(&id) && !self.humans.contains_key(&id) {
                missing_prompt_ids.push(id);
            }
        }

        // Load missing prompts in parallel
        if !missing_prompt_ids.is_empty() {
            let prompts = self.load_prompts_concurrent(&missing_prompt_ids).await?;
            for (id, commit_sha, prompt) in prompts {
                self.prompts
                    .entry(id)
                    .or_default()
                    .insert(commit_sha, prompt);
            }
        }

        // Load missing sessions from history
        if !missing_session_ids.is_empty() {
            let sessions = self
                .load_sessions_concurrent(&missing_session_ids.into_iter().collect::<Vec<_>>())
                .await?;
            for (session_id, session_record) in sessions {
                self.sessions.entry(session_id).or_insert(session_record);
            }
        }

        Ok(())
    }

    /// Load multiple prompts concurrently using MAX_CONCURRENT limit
    async fn load_prompts_concurrent(
        &self,
        missing_ids: &[String],
    ) -> Result<Vec<(String, String, PromptRecord)>, GitAiError> {
        const MAX_CONCURRENT: usize = 30;

        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT));
        let mut tasks = Vec::new();

        for missing_id in missing_ids {
            let missing_id = missing_id.clone();
            let repo = self.repo.clone();
            let semaphore = Arc::clone(&semaphore);

            let task = async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("prompt lookup semaphore was closed");

                crate::tokio_runtime::spawn_blocking_result(move || {
                    Self::find_prompt_in_history_static(&repo, &missing_id)
                        .map(|(commit_sha, prompt)| (missing_id, commit_sha, prompt))
                })
                .await
            };

            tasks.push(task);
        }

        // Await all tasks concurrently
        let results = futures::future::join_all(tasks).await;

        // Process results and collect successful prompts
        let mut prompts = Vec::new();
        for result in results {
            match result {
                Ok((id, commit_sha, prompt)) => prompts.push((id, commit_sha, prompt)),
                Err(_) => {
                    // Error finding prompt, skip it
                }
            }
        }

        Ok(prompts)
    }

    /// Static version of find_prompt_in_history for use in async context
    /// Returns (commit_sha, PromptRecord) for the most recent commit containing this prompt
    fn find_prompt_in_history_static(
        repo: &Repository,
        prompt_id: &str,
    ) -> Result<(String, crate::model::authorship_log::PromptRecord), GitAiError> {
        // Use git grep to search for the prompt ID in authorship notes
        let shas =
            crate::operations::git::notes_api::search_notes(repo, &format!("\"{}\"", prompt_id))
                .unwrap_or_default();

        // Check the most recent commit with this prompt ID
        if let Some(latest_sha) = shas.first()
            && let Ok(log) = crate::operations::git::notes_api::read_authorship_v3(repo, latest_sha)
            && let Some(prompt) = log.metadata.prompts.get(prompt_id)
        {
            return Ok((latest_sha.clone(), prompt.clone()));
        }

        Err(GitAiError::Generic(format!(
            "Prompt not found in history: {}",
            prompt_id
        )))
    }

    /// Load multiple sessions concurrently from git note history
    async fn load_sessions_concurrent(
        &self,
        missing_ids: &[String],
    ) -> Result<Vec<(String, SessionRecord)>, GitAiError> {
        const MAX_CONCURRENT: usize = 30;

        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT));
        let mut tasks = Vec::new();

        for missing_id in missing_ids {
            let missing_id = missing_id.clone();
            let repo = self.repo.clone();
            let semaphore = Arc::clone(&semaphore);

            let task = async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("session lookup semaphore was closed");
                crate::tokio_runtime::spawn_blocking_result(move || {
                    Self::find_session_in_history_static(&repo, &missing_id)
                        .map(|record| (missing_id, record))
                })
                .await
            };

            tasks.push(task);
        }

        let results = futures::future::join_all(tasks).await;
        let sessions: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
        Ok(sessions)
    }

    fn find_session_in_history_static(
        repo: &Repository,
        session_id: &str,
    ) -> Result<SessionRecord, GitAiError> {
        // Go through notes_api (not refs::) so the HTTP notes backend is honored.
        let shas =
            crate::operations::git::notes_api::search_notes(repo, &format!("\"{}\"", session_id))
                .unwrap_or_default();

        if let Some(latest_sha) = shas.first()
            && let Ok(log) = crate::operations::git::notes_api::read_authorship_v3(repo, latest_sha)
            && let Some(session) = log.metadata.sessions.get(session_id)
        {
            return Ok(session.clone());
        }

        Err(GitAiError::Generic(format!(
            "Session not found in history: {}",
            session_id
        )))
    }
}
