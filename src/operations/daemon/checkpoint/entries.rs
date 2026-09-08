use super::files::{build_previous_file_state_maps, get_checkpoint_entry_for_file};
use super::metrics::FileLineStats;
use crate::error::GitAiError;
use crate::model::attribution_tracker::LineAttribution;
use crate::model::authorship_log_serialization::generate_session_id;
use crate::model::checkpoint_request::CheckpointRequest;
use crate::model::working_log::{Checkpoint, CheckpointKind, WorkingLogEntry};
use crate::operations::git::repo_storage::PersistedWorkingLog;
use crate::operations::git::repository::Repository;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[allow(clippy::too_many_arguments)]
pub(super) async fn get_checkpoint_entries(
    kind: CheckpointKind,
    author: &str,
    repo: &Repository,
    working_log: &PersistedWorkingLog,
    files: &[String],
    file_content_hashes: &HashMap<String, String>,
    previous_checkpoints: &[Checkpoint],
    checkpoint_request: &CheckpointRequest,
    ts: u128,
    head_commit_override: Option<&str>,
    trace_id: String,
) -> Result<(Vec<WorkingLogEntry>, Vec<FileLineStats>), GitAiError> {
    let entries_fn_start = Instant::now();

    // Read INITIAL attributions from working log (empty if file doesn't exist)
    let initial_read_start = Instant::now();
    let initial_data = working_log.read_initial_attributions();
    let initial_snapshot_contents: HashMap<String, Arc<str>> = {
        let mut map = HashMap::new();
        for file_path in initial_data.files.keys() {
            if let Some(content) =
                working_log.initial_file_content_from(&initial_data, file_path)?
            {
                map.insert(file_path.clone(), Arc::<str>::from(content));
            }
        }
        map
    };
    let initial_attributions = initial_data.files;
    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK] Reading initial attributions took {:?}",
        initial_read_start.elapsed()
    );

    let precompute_start = Instant::now();
    let (previous_file_state_by_file, ai_touched_files) =
        build_previous_file_state_maps(previous_checkpoints, &initial_attributions);
    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK] Precomputing previous state maps took {:?}",
        precompute_start.elapsed()
    );

    // Determine author_id based on checkpoint kind and agent_id
    let author_id = match kind {
        CheckpointKind::Human => kind.to_str(), // "human" — stripped, never attested
        CheckpointKind::KnownHuman => {
            crate::model::authorship_log_serialization::generate_human_short_hash(author)
        }
        _ => {
            // AI kinds: compose session_id::trace_id
            checkpoint_request
                .agent_id
                .as_ref()
                .map(|aid| {
                    let session_id = generate_session_id(&aid.id, &aid.tool);
                    format!("{}::{}", session_id, trace_id)
                })
                .unwrap_or_else(|| kind.to_str())
        }
    };

    // Get HEAD commit info for git operations
    let head_commit = head_commit_override
        .map(str::trim)
        .filter(|sha| !sha.is_empty() && *sha != "initial")
        .and_then(|sha| repo.find_commit(sha.to_string()).ok())
        .or_else(|| {
            repo.head()
                .ok()
                .and_then(|h| h.target().ok())
                .and_then(|oid| repo.find_commit(oid).ok())
        });
    let head_tree_id = head_commit
        .as_ref()
        .and_then(|c| c.tree().ok())
        .map(|t| t.id().to_string());

    let parent_note_attributions: HashMap<String, Vec<LineAttribution>> = HashMap::new();

    const MAX_CONCURRENT: usize = 30;

    // Create a semaphore to limit concurrent tasks
    let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT));

    // Move other repeated allocations outside the loop
    let previous_file_state_by_file = Arc::new(previous_file_state_by_file);
    let ai_touched_files = Arc::new(ai_touched_files);
    let author_id = Arc::new(author_id);
    let head_tree_id = Arc::new(head_tree_id);
    let initial_attributions = Arc::new(initial_attributions);
    let initial_snapshot_contents = Arc::new(initial_snapshot_contents);
    let parent_note_attributions = Arc::new(parent_note_attributions);

    // Spawn tasks for each file
    let spawn_start = Instant::now();
    let mut tasks = Vec::new();

    for file_path in files {
        let file_path = file_path.clone();
        let repo = repo.clone();
        let working_log = working_log.clone();
        let previous_file_state_by_file = Arc::clone(&previous_file_state_by_file);
        let ai_touched_files = Arc::clone(&ai_touched_files);
        let author_id = Arc::clone(&author_id);
        let head_tree_id = Arc::clone(&head_tree_id);
        let blob_sha = file_content_hashes
            .get(&file_path)
            .cloned()
            .unwrap_or_default();
        let initial_attributions = Arc::clone(&initial_attributions);
        let initial_snapshot_contents = Arc::clone(&initial_snapshot_contents);
        let parent_note_attributions = Arc::clone(&parent_note_attributions);
        let semaphore = Arc::clone(&semaphore);

        let task = async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .expect("checkpoint entry semaphore was closed");

            crate::tokio_runtime::spawn_blocking_result(move || {
                get_checkpoint_entry_for_file(
                    file_path,
                    kind,
                    repo,
                    working_log,
                    previous_file_state_by_file,
                    ai_touched_files,
                    blob_sha,
                    author_id.clone(),
                    head_tree_id.clone(),
                    initial_attributions.clone(),
                    initial_snapshot_contents.clone(),
                    parent_note_attributions.clone(),
                    ts,
                )
            })
            .await
        };

        tasks.push(task);
    }
    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK] Spawning {} tasks took {:?}",
        tasks.len(),
        spawn_start.elapsed()
    );

    // Await all tasks concurrently
    let await_start = Instant::now();
    let results = futures::future::join_all(tasks).await;
    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK] Awaiting {} tasks took {:?}",
        results.len(),
        await_start.elapsed()
    );

    // Process results
    let process_start = Instant::now();
    let results_count = results.len();
    let mut entries = Vec::new();
    let mut file_stats = Vec::new();
    for result in results {
        match result {
            Ok(Some((entry, stats))) => {
                entries.push(entry);
                file_stats.push(stats);
            }
            Ok(None) => {} // File had no changes
            Err(e) => return Err(e),
        }
    }
    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK] Processing {} results took {:?}",
        results_count,
        process_start.elapsed()
    );
    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK] get_checkpoint_entries function total took {:?}",
        entries_fn_start.elapsed()
    );

    Ok((entries, file_stats))
}
