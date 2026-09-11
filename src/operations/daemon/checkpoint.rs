use crate::error::GitAiError;
#[cfg(not(any(test, feature = "test-support")))]
use crate::model::authorship_log_serialization::generate_short_hash;
use crate::model::checkpoint_request::CheckpointRequest;
pub use crate::model::checkpoint_request::PreparedPathRole;
use crate::model::working_log::{Checkpoint, CheckpointKind};
use crate::operations::git::repository::Repository;
use sha2::{Digest, Sha256};
use std::time::Instant;

mod files;
mod metrics;
mod types;

pub use files::is_ai_author_id;
use files::{checkpoint_error, get_checkpoint_entries, save_current_file_states};
pub use metrics::{build_agent_usage_attrs, compute_file_line_stats};
use metrics::{build_checkpoint_attrs, compute_line_stats};
pub use types::{FileLineStats, ResolvedCheckpointExecution};

use crate::model::working_log::AgentId;

#[cfg(not(any(test, feature = "test-support")))]
const KNOWN_HUMAN_MIN_SECS_AFTER_AI: u64 = 1;

#[cfg(not(any(test, feature = "test-support")))]
pub(crate) fn should_emit_agent_usage(agent_id: &AgentId) -> bool {
    let prompt_id = generate_short_hash(&agent_id.id, &agent_id.tool);
    crate::operations::daemon::agent_usage_limiter::should_emit(&prompt_id)
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn should_emit_agent_usage(_agent_id: &AgentId) -> bool {
    false
}

pub fn execute_resolved_checkpoint_from_daemon(
    repo: &Repository,
    author: &str,
    kind: CheckpointKind,
    checkpoint_request: CheckpointRequest,
    resolved: ResolvedCheckpointExecution,
) -> Result<(), GitAiError> {
    let checkpoint_start = Instant::now();
    tracing::debug!("[BENCHMARK] Starting daemon replay checkpoint");
    execute_resolved_checkpoint(
        repo,
        author,
        kind,
        true,
        checkpoint_request,
        resolved,
        checkpoint_start,
    )
    .map(|_| ())
}

fn execute_resolved_checkpoint(
    repo: &Repository,
    author: &str,
    kind: CheckpointKind,
    quiet: bool,
    checkpoint_request: CheckpointRequest,
    mut resolved: ResolvedCheckpointExecution,
    checkpoint_start: Instant,
) -> Result<(usize, usize, usize), GitAiError> {
    if kind.is_ai() && checkpoint_request.agent_id.is_none() {
        return Err(checkpoint_error(
            std::io::ErrorKind::InvalidData,
            "AI checkpoint is missing agent_id".to_string(),
        ));
    }

    let mut working_log = repo
        .storage
        .working_log_for_base_commit(&resolved.base_commit)?;

    if !resolved.dirty_files.is_empty() {
        working_log.set_dirty_files(Some(std::mem::take(&mut resolved.dirty_files)));
    }

    let read_checkpoints_start = Instant::now();
    let mut checkpoints = working_log.load_cached_checkpoint_journal()?;
    tracing::debug!(
        "[BENCHMARK] Reading {} checkpoints took {:?}",
        checkpoints.len(),
        read_checkpoints_start.elapsed()
    );

    // At-least-once outbox replay: a delivery whose id is already recorded in
    // the working log was fully applied before — drop the duplicate before
    // doing any file-state work.
    if let Some(delivery_id) = checkpoint_request.delivery_id.as_deref()
        && let Some(applied_checkpoint_index) = checkpoints
            .iter()
            .position(|cp| cp.delivery_id.as_deref() == Some(delivery_id))
    {
        // A prior append may have reached the page cache before returning an
        // fsync error. Make that complete, checksummed record durable before
        // an outbox replay treats the delivery as safely applied.
        working_log
            .ensure_cached_checkpoint_record_durable(&mut checkpoints, applied_checkpoint_index)?;
        tracing::debug!(delivery_id, "skipping already-applied checkpoint delivery");
        return Ok((0, resolved.files.len(), checkpoints.len()));
    }

    // Reject KnownHuman checkpoints that arrive within KNOWN_HUMAN_MIN_SECS_AFTER_AI
    // seconds of an AI checkpoint on any of the same files. These are likely spurious
    // IDE save events triggered by the AI completing its edit, not genuine human keystrokes.
    // Only compiled in non-test builds where the constant is non-zero; under --all-targets
    // clippy would otherwise flag the comparisons as always-false for u64.
    #[cfg(not(any(test, feature = "test-support")))]
    if kind == CheckpointKind::KnownHuman {
        let now_secs = crate::model::clock::now_secs();
        let too_soon = checkpoints.iter().rev().any(|cp| {
            cp.kind.is_ai()
                && now_secs.saturating_sub(cp.timestamp) < KNOWN_HUMAN_MIN_SECS_AFTER_AI
                && cp.entries.iter().any(|e| resolved.files.contains(&e.file))
        });
        if too_soon {
            tracing::debug!(
                "[KnownHuman] Rejected: fired within {}s of an AI checkpoint on the same file",
                KNOWN_HUMAN_MIN_SECS_AFTER_AI
            );
            return Ok((0, 0, 0));
        }
    }

    let save_states_start = Instant::now();
    let file_content_hashes = save_current_file_states(&working_log, &resolved.files)?;
    tracing::debug!(
        "[BENCHMARK] save_current_file_states for {} files took {:?}",
        resolved.files.len(),
        save_states_start.elapsed()
    );

    let hash_compute_start = Instant::now();
    let mut ordered_hashes: Vec<_> = file_content_hashes.iter().collect();
    ordered_hashes.sort_by_key(|(file_path, _)| *file_path);

    let mut combined_hasher = Sha256::new();
    for (file_path, hash) in ordered_hashes {
        combined_hasher.update(file_path.as_bytes());
        combined_hasher.update(hash.as_bytes());
    }
    let combined_hash = format!("{:x}", combined_hasher.finalize());
    tracing::debug!(
        "[BENCHMARK] Hash computation took {:?}",
        hash_compute_start.elapsed()
    );

    let trace_id = checkpoint_request.trace_id.clone();

    let entries_start = Instant::now();
    let (entries, file_stats) = crate::tokio_runtime::block_on(get_checkpoint_entries(
        kind,
        author,
        repo,
        &working_log,
        &resolved.files,
        &file_content_hashes,
        &checkpoints,
        &checkpoint_request,
        resolved.ts,
        Some(resolved.base_commit.as_str()),
        trace_id.clone(),
    ))?;
    tracing::debug!(
        "[BENCHMARK] get_checkpoint_entries generated {} entries, took {:?}",
        entries.len(),
        entries_start.elapsed()
    );

    let entry_count = entries.len();
    if !entries.is_empty() {
        let checkpoint_create_start = Instant::now();
        let mut checkpoint = Checkpoint::new(kind, combined_hash, author.to_string(), entries);
        checkpoint.timestamp = (resolved.ts / 1000) as u64;
        checkpoint.line_stats = compute_line_stats(&file_stats)?;
        checkpoint.trace_id = Some(trace_id.clone());
        checkpoint.delivery_id = checkpoint_request.delivery_id.clone();

        if kind.is_ai() {
            checkpoint.agent_id = checkpoint_request.agent_id.clone();
            checkpoint.agent_metadata = if checkpoint_request.metadata.is_empty() {
                None
            } else {
                Some(checkpoint_request.metadata.clone())
            };
        } else if kind == CheckpointKind::KnownHuman && !checkpoint_request.metadata.is_empty() {
            let editor = checkpoint_request
                .metadata
                .get("kh_editor")
                .cloned()
                .unwrap_or_default();
            let editor_version = checkpoint_request
                .metadata
                .get("kh_editor_version")
                .cloned()
                .unwrap_or_default();
            let extension_version = checkpoint_request
                .metadata
                .get("kh_extension_version")
                .cloned()
                .unwrap_or_default();
            if !editor.is_empty() {
                use crate::model::working_log::KnownHumanMetadata;
                checkpoint.known_human_metadata = Some(KnownHumanMetadata {
                    editor,
                    editor_version,
                    extension_version,
                });
            }
        }
        tracing::debug!(
            "[BENCHMARK] Checkpoint creation took {:?}",
            checkpoint_create_start.elapsed()
        );

        let append_start = Instant::now();
        // Reuses the checkpoint collection materialized above instead of
        // re-reading the working log, and moves the checkpoint in.
        working_log.append_cached_checkpoint_record_to(&mut checkpoints, checkpoint)?;
        tracing::debug!(
            "[BENCHMARK] Appending checkpoint to working log took {:?}",
            append_start.elapsed()
        );
        let checkpoint = checkpoints
            .last()
            .expect("checkpoint was appended to the collection above");

        let mut attrs =
            build_checkpoint_attrs(repo, &resolved.base_commit, checkpoint.agent_id.as_ref());

        // Add trace_id to attributes - links all checkpoint events together
        if let Some(ref tid) = checkpoint.trace_id {
            attrs = attrs.trace_id(tid);
        }

        // Extract tool_use_id from metadata if available
        // tool_use_id tracks specific tool invocations (e.g., bash tool calls from AI agents)
        // Allows linking checkpoint events to the exact tool use that triggered them
        let tool_use_id = checkpoint_request
            .metadata
            .get("tool_use_id")
            .map(|s| s.as_str());

        let edit_kind = checkpoint_request
            .metadata
            .get("edit_kind")
            .map(|s| s.as_str());

        for (entry, file_stat) in checkpoint.entries.iter().zip(file_stats.iter()) {
            let mut values = crate::metrics::CheckpointValues::new()
                .checkpoint_ts(checkpoint.timestamp)
                .kind(checkpoint.kind.to_str().to_string())
                .file_path(entry.file.clone())
                .lines_added(file_stat.additions)
                .lines_deleted(file_stat.deletions)
                .lines_added_sloc(file_stat.additions_sloc)
                .lines_deleted_sloc(file_stat.deletions_sloc);

            if let Some(tuid) = tool_use_id {
                values = values.external_tool_use_id(tuid);
            }
            if let Some(ek) = edit_kind {
                values = values.edit_kind(ek);
            }

            let file_attrs = attrs.clone().author(&checkpoint.author);
            crate::metrics::record(values, file_attrs);
        }
    }

    let agent_tool = if kind.is_ai() {
        checkpoint_request
            .agent_id
            .as_ref()
            .map(|aid| aid.tool.as_str())
    } else {
        None
    };

    let label = if entry_count > 1 {
        "checkpoint"
    } else {
        "commit"
    };

    if !quiet {
        let log_author = agent_tool.unwrap_or(author);
        let files_with_entries = entry_count;
        let total_uncommitted_files = resolved.files.len();

        if files_with_entries == total_uncommitted_files {
            eprintln!(
                "{} {} changed {} file(s) that have changed since the last {}",
                kind.to_str(),
                log_author,
                files_with_entries,
                label
            );
        } else {
            eprintln!(
                "{} {} changed {} of the {} file(s) that have changed since the last {} ({} already checkpointed)",
                kind.to_str(),
                log_author,
                files_with_entries,
                total_uncommitted_files,
                label,
                total_uncommitted_files - files_with_entries
            );
        }
    }

    tracing::debug!(
        "[BENCHMARK] Total checkpoint run took {:?}",
        checkpoint_start.elapsed()
    );
    Ok((entry_count, resolved.files.len(), checkpoints.len()))
}
