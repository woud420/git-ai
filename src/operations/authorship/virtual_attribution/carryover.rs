use super::merge::merge_attributions_favoring_first;
use super::types::VirtualAttributions;
use crate::error::GitAiError;
use crate::operations::git::repository::Repository;
use std::collections::HashMap;

pub fn restore_working_log_carryover(
    repo: &Repository,
    old_head: &str,
    new_head: &str,
    final_state: HashMap<String, String>,
    human_author: Option<String>,
) -> Result<(), GitAiError> {
    if old_head.is_empty() || new_head.is_empty() || final_state.is_empty() {
        return Ok(());
    }

    let old_va = VirtualAttributions::from_persisted_working_log(
        repo.clone(),
        old_head.to_string(),
        human_author,
    )?;
    restore_virtual_attribution_carryover(repo, new_head, old_va, final_state)
}

pub fn restore_virtual_attribution_carryover(
    repo: &Repository,
    new_head: &str,
    carried_va: VirtualAttributions,
    final_state: HashMap<String, String>,
) -> Result<(), GitAiError> {
    if new_head.is_empty() || final_state.is_empty() || carried_va.attributions.is_empty() {
        return Ok(());
    }

    let new_va =
        VirtualAttributions::from_persisted_working_log(repo.clone(), new_head.to_string(), None)
            .unwrap_or_else(|_| {
                VirtualAttributions::new(
                    repo.clone(),
                    new_head.to_string(),
                    HashMap::new(),
                    HashMap::new(),
                    0,
                )
            });

    let merged_va = merge_attributions_favoring_first(carried_va, new_va, final_state.clone())?;
    let initial_attributions = merged_va.to_initial_working_log_only();
    if initial_attributions.files.is_empty()
        && initial_attributions.prompts.is_empty()
        && initial_attributions.sessions.is_empty()
    {
        return Ok(());
    }

    let working_log = repo.storage.working_log_for_base_commit(new_head)?;
    working_log.write_initial_attributions_with_contents(
        initial_attributions.files,
        initial_attributions.prompts,
        initial_attributions.humans,
        final_state,
        initial_attributions.sessions,
    )?;
    Ok(())
}
