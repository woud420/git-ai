use crate::model::working_log::Checkpoint;
use std::collections::HashMap;

const KNOWN_HUMAN_DUPLICATE_WINDOW_SECS: u64 = 3;

pub(super) fn filter_same_content_known_human_files(
    files: &mut Vec<String>,
    current_hashes: &HashMap<String, String>,
    checkpoints: &[Checkpoint],
    now_secs: u64,
) -> usize {
    let mut latest_ai_state_by_file = HashMap::new();
    for checkpoint in checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.kind.is_ai())
    {
        for entry in &checkpoint.entries {
            latest_ai_state_by_file.insert(
                entry.file.as_str(),
                (entry.blob_sha.as_str(), checkpoint.timestamp),
            );
        }
    }

    let original_len = files.len();
    files.retain(|file| {
        let is_same_recent_ai_content = current_hashes
            .get(file)
            .zip(latest_ai_state_by_file.get(file.as_str()))
            .is_some_and(|(current_hash, (ai_hash, timestamp))| {
                current_hash == ai_hash
                    && now_secs.saturating_sub(*timestamp) <= KNOWN_HUMAN_DUPLICATE_WINDOW_SECS
            });
        !is_same_recent_ai_content
    });
    original_len - files.len()
}
