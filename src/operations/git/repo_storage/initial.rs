use super::{PersistedWorkingLog, persistence_error};
use crate::error::GitAiError;
use crate::model::attribution_tracker::LineAttribution;
use crate::model::authorship_log::{HumanRecord, PromptRecord, SessionRecord};
use crate::model::working_log::InitialAttributions;
use std::collections::HashMap;

impl PersistedWorkingLog {
    pub(crate) fn prepare_initial_attributions_with_contents(
        &self,
        attributions: HashMap<String, Vec<LineAttribution>>,
        prompts: HashMap<String, PromptRecord>,
        humans: std::collections::BTreeMap<String, HumanRecord>,
        file_contents: HashMap<String, String>,
        sessions: std::collections::BTreeMap<String, SessionRecord>,
    ) -> Result<InitialAttributions, GitAiError> {
        let filtered: HashMap<String, Vec<LineAttribution>> = attributions
            .into_iter()
            .filter(|(_, attrs)| !attrs.is_empty())
            .collect();
        let mut file_blobs = HashMap::new();
        for file_path in filtered.keys() {
            let content = file_contents.get(file_path).ok_or_else(|| {
                persistence_error(
                    std::io::ErrorKind::NotFound,
                    format!("INITIAL missing file content snapshot for {}", file_path),
                )
            })?;
            let blob_sha = self.persist_file_version(content)?;
            file_blobs.insert(file_path.clone(), blob_sha);
        }

        Ok(InitialAttributions {
            files: filtered,
            prompts,
            file_blobs,
            humans,
            sessions,
        })
    }
}
