use super::metrics::{FileLineStats, compute_file_line_stats};
use crate::error::GitAiError;
use crate::model::attribution_tracker::{
    Attribution, AttributionTracker, INITIAL_ATTRIBUTION_TS, LineAttribution,
};
use crate::model::imara_diff_utils::content_eq_ignoring_line_endings;
use crate::model::repository::error::PersistenceError;
use crate::model::working_log::{Checkpoint, CheckpointKind, WorkingLogEntry};
use crate::operations::git::repo_storage::{PersistedWorkingLog, persist_file_version_to_blob_dir};
use crate::operations::git::repository::Repository;
use futures::stream::{self, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

/// Latest checkpoint state needed to process a file in the next checkpoint.
#[derive(Debug, Clone)]
pub(super) struct PreviousFileState {
    blob_sha: String,
    attributions: Vec<Attribution>,
}

pub(super) fn checkpoint_error(kind: std::io::ErrorKind, message: String) -> GitAiError {
    PersistenceError::Io {
        operation: "Generic error",
        path: String::new(),
        kind,
        message,
    }
    .into()
}

pub(super) fn save_current_file_states(
    working_log: &PersistedWorkingLog,
    files: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    let _read_start = Instant::now();

    let blobs_dir = working_log.dir.join("blobs");
    let dirty_files = working_log.dirty_files.clone();
    let files = files.to_vec();

    let file_content_hashes = crate::tokio_runtime::block_on(async {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(8));
        let blobs_dir = Arc::new(blobs_dir);
        let dirty_files = Arc::new(dirty_files);

        let mut futures = Vec::with_capacity(files.len());
        for file_path in files {
            let blobs_dir = Arc::clone(&blobs_dir);
            let dirty_files = Arc::clone(&dirty_files);
            let semaphore = Arc::clone(&semaphore);

            futures.push(async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("file state semaphore was closed");

                // Read file content - check dirty_files first, then filesystem
                let content = if let Some(ref dirty_map) = *dirty_files {
                    dirty_map.get(&file_path).cloned()
                } else {
                    None
                }
                .ok_or_else(|| {
                    checkpoint_error(
                        std::io::ErrorKind::NotFound,
                        format!(
                            "save_current_file_states: file '{}' not found in dirty_files snapshot (filesystem fallback is not allowed in checkpoint flow)",
                            file_path
                        ),
                    )
                })?;

                crate::tokio_runtime::spawn_blocking_result(move || {
                    let sha =
                        persist_file_version_to_blob_dir(blobs_dir.as_ref(), content.as_ref())?;

                    Ok::<(String, String), GitAiError>((file_path, sha))
                })
                .await
            });
        }

        // Collect results from all concurrent operations
        let results: Vec<Result<(String, String), GitAiError>> =
            stream::iter(futures).buffer_unordered(8).collect().await;

        // Convert results into HashMap
        let mut file_content_hashes = HashMap::new();
        for result in results {
            let (file_path, content_hash) = result?;
            file_content_hashes.insert(file_path, content_hash);
        }

        Ok::<HashMap<String, String>, GitAiError>(file_content_hashes)
    })?;

    Ok(file_content_hashes)
}

fn get_previous_content_from_head(
    repo: &Repository,
    file_path: &str,
    head_tree_id: &Option<String>,
) -> Arc<str> {
    let Some(tree_id) = head_tree_id.as_ref() else {
        return Arc::from("");
    };
    match repo.read_file_blob_at_tree(tree_id, std::path::Path::new(file_path)) {
        Ok(content) => {
            let text = String::from_utf8_lossy(&content);
            Arc::from(text.into_owned())
        }
        Err(_) => Arc::from(""),
    }
}

#[doc(hidden)]
pub fn is_ai_author_id(author_id: &str) -> bool {
    author_id != "human" && !author_id.starts_with("h_")
}

fn working_log_entry_has_non_human_attribution(entry: &WorkingLogEntry) -> bool {
    entry
        .line_attributions
        .iter()
        .any(|attr| is_ai_author_id(&attr.author_id))
        || entry
            .attributions
            .iter()
            .any(|attr| is_ai_author_id(&attr.author_id))
}

pub(super) fn build_previous_file_state_maps(
    previous_checkpoints: &[Checkpoint],
    initial_attributions: &HashMap<String, Vec<LineAttribution>>,
) -> (HashMap<String, PreviousFileState>, HashSet<String>) {
    let mut previous_file_state_by_file: HashMap<String, PreviousFileState> = HashMap::new();
    let mut ai_touched_files: HashSet<String> = initial_attributions.keys().cloned().collect();

    // Keep only the latest entry for each file.
    for checkpoint in previous_checkpoints {
        for entry in &checkpoint.entries {
            previous_file_state_by_file.insert(
                entry.file.clone(),
                PreviousFileState {
                    blob_sha: entry.blob_sha.clone(),
                    attributions: entry.attributions.clone(),
                },
            );

            if checkpoint.kind.is_ai() || working_log_entry_has_non_human_attribution(entry) {
                ai_touched_files.insert(entry.file.clone());
            }
        }
    }

    (previous_file_state_by_file, ai_touched_files)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn get_checkpoint_entry_for_file(
    file_path: String,
    kind: CheckpointKind,
    repo: Repository,
    working_log: PersistedWorkingLog,
    previous_file_state_by_file: Arc<HashMap<String, PreviousFileState>>,
    ai_touched_files: Arc<HashSet<String>>,
    file_content_hash: String,
    author_id: Arc<String>,
    head_tree_id: Arc<Option<String>>,
    initial_attributions: Arc<HashMap<String, Vec<LineAttribution>>>,
    initial_snapshot_contents: Arc<HashMap<String, Arc<str>>>,
    parent_note_attributions: Arc<HashMap<String, Vec<LineAttribution>>>,
    ts: u128,
) -> Result<Option<(WorkingLogEntry, FileLineStats)>, GitAiError> {
    // Deterministic blocking work for the TestRepo throughput guard; release builds omit it.
    #[cfg(feature = "test-support")]
    if let Some(delay_millis) = std::env::var_os("GIT_AI_TEST_CHECKPOINT_FILE_DELAY_MS")
        .and_then(|value| value.to_str().and_then(|value| value.parse::<u64>().ok()))
    {
        std::thread::sleep(std::time::Duration::from_millis(delay_millis));
    }

    let file_start = Instant::now();
    let initial_attrs_for_file = initial_attributions
        .get(&file_path)
        .cloned()
        .unwrap_or_default();
    let initial_snapshot_content = initial_snapshot_contents.get(&file_path).cloned();

    let previous_state = previous_file_state_by_file.get(&file_path).cloned();
    let has_prior_ai_edits = ai_touched_files.contains(&file_path);

    let current_content = working_log
        .read_current_file_content(&file_path)
        .unwrap_or_else(|_| Arc::<str>::from(""));

    // Non-pre-commit fast path:
    // Preserve existing `git-ai checkpoint` behavior for human-only files by writing an
    // attribution-empty entry while still capturing line stats.
    // KnownHuman checkpoints must bypass this path so they record h_<hash> attributions
    // that later AI checkpoints can use to identify human-written lines.
    if kind == CheckpointKind::Human && !has_prior_ai_edits && initial_attrs_for_file.is_empty() {
        let previous_content = if let Some(state) = previous_state.as_ref() {
            Arc::<str>::from(
                working_log
                    .get_file_version(&state.blob_sha)
                    .unwrap_or_default(),
            )
        } else {
            get_previous_content_from_head(&repo, &file_path, head_tree_id.as_ref())
        };

        if content_eq_ignoring_line_endings(&current_content, &previous_content) {
            return Ok(None);
        }

        let stats = compute_file_line_stats(&previous_content, &current_content);
        let entry = WorkingLogEntry::new(file_path, file_content_hash, Vec::new(), Vec::new());
        return Ok(Some((entry, stats)));
    }

    let from_checkpoint = previous_state.as_ref().map(|state| {
        (
            Arc::<str>::from(
                working_log
                    .get_file_version(&state.blob_sha)
                    .unwrap_or_default(),
            ),
            state.attributions.clone(),
        )
    });

    let is_from_checkpoint = from_checkpoint.is_some();
    let (previous_content, prev_attributions) = if let Some((content, attrs)) = from_checkpoint {
        (content, attrs)
    } else {
        // File doesn't exist in any previous checkpoint - need to initialize from git + INITIAL
        let previous_content =
            get_previous_content_from_head(&repo, &file_path, head_tree_id.as_ref());

        // Skip if no changes, UNLESS we have INITIAL attributions for this file
        // (in which case we need to create an entry to record those attributions)
        if content_eq_ignoring_line_endings(&current_content, &previous_content)
            && initial_attrs_for_file.is_empty()
        {
            return Ok(None);
        }

        // Build a set of lines covered by INITIAL attributions
        let mut initial_covered_lines: HashSet<u32> = HashSet::new();
        for attr in &initial_attrs_for_file {
            for line in attr.start_line..=attr.end_line {
                initial_covered_lines.insert(line);
            }
        }

        // Start with INITIAL attributions (they win), augmented by parent note
        let mut prev_line_attributions = initial_attrs_for_file.clone();

        // Parent note seeding removed — handled at post-commit via inheritance.
        let _ = &parent_note_attributions;

        let mut blamed_lines: HashSet<u32> = HashSet::new();

        // Default all previous-content lines to "human" (no cross-commit blame).
        // When INITIAL has a snapshot that DIFFERS from current content, use its
        // line count (that's what the diff will compare against). When the snapshot
        // matches current content (no edits after INITIAL), use the HEAD content
        // line count so the AI fallback can fire for uncovered lines.
        let effective_prev_content = if !initial_attrs_for_file.is_empty() {
            let snapshot = initial_snapshot_content
                .as_deref()
                .unwrap_or(&previous_content);
            if content_eq_ignoring_line_endings(snapshot, &current_content) {
                &previous_content
            } else {
                snapshot
            }
        } else {
            &previous_content
        };
        let prev_total_lines = effective_prev_content.lines().count() as u32;
        for line_num in 1..=prev_total_lines {
            blamed_lines.insert(line_num);
        }

        // For AI checkpoints, attribute any lines NOT in INITIAL and NOT returned by ai_blame
        if kind.is_ai() {
            let total_lines = current_content.lines().count() as u32;
            for line_num in 1..=total_lines {
                if !initial_covered_lines.contains(&line_num) && !blamed_lines.contains(&line_num) {
                    prev_line_attributions.push(LineAttribution {
                        start_line: line_num,
                        end_line: line_num,
                        author_id: author_id.as_ref().clone(),
                        overrode: None,
                    });
                }
            }
        }

        // INITIAL line numbers refer to the file state at the moment INITIAL was written.
        // Snapshot-aware INITIAL storage preserves that exact content; older INITIAL files
        // fall back to the legacy "current content" behavior.
        let content_for_line_conversion = if !initial_attrs_for_file.is_empty() {
            initial_snapshot_content
                .as_deref()
                .unwrap_or(&current_content)
        } else {
            &previous_content
        };

        // Convert any line attributions to character attributions
        let prev_attributions =
            crate::model::attribution_tracker::line_attributions_to_attributions(
                &prev_line_attributions,
                content_for_line_conversion,
                INITIAL_ATTRIBUTION_TS,
            );

        // When INITIAL has a persisted snapshot, use that as the previous content so later
        // edits after a restore/squash are tracked correctly. Older INITIAL files fall back
        // to the legacy current-content behavior.
        let adjusted_previous = if !initial_attrs_for_file.is_empty() {
            initial_snapshot_content.unwrap_or_else(|| current_content.clone())
        } else {
            previous_content
        };

        (adjusted_previous, prev_attributions)
    };

    // Skip if no changes (but we already checked this earlier, accounting for INITIAL attributions)
    // For files from previous checkpoints, check if content has changed
    if is_from_checkpoint && content_eq_ignoring_line_endings(&current_content, &previous_content) {
        if current_content == previous_content {
            // Byte-identical — truly no change.
            return Ok(None);
        }
        // Content differs only in line endings (CRLF ↔ LF). Update the stored blob
        // to the current content so future diffs compare LF-vs-LF. Without this,
        // the stale CRLF blob causes capture_diff_slices to see every line as changed,
        // and AI checkpoints (force_split=true) would re-attribute all lines to AI.
        // Remap attributions through line-number space to adjust byte offsets.
        let line_attributions =
            crate::model::attribution_tracker::attributions_to_line_attributions_for_checkpoint(
                &prev_attributions,
                &previous_content,
                kind.is_ai(),
            );
        let remapped_attributions =
            crate::model::attribution_tracker::line_attributions_to_attributions(
                &line_attributions,
                &current_content,
                ts,
            );
        let entry = WorkingLogEntry::new(
            file_path,
            file_content_hash,
            remapped_attributions,
            line_attributions,
        );
        return Ok(Some((entry, FileLineStats::default())));
    }

    let (entry, stats) = make_entry_for_file(FileEntryInput {
        file_path: &file_path,
        blob_sha: &file_content_hash,
        author_id: author_id.as_ref(),
        is_ai_checkpoint: kind.is_ai(),
        previous_content: &previous_content,
        previous_attributions: &prev_attributions,
        content: &current_content,
        ts,
    })?;

    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK] Processing file {} took {:?}",
        file_path,
        file_start.elapsed()
    );
    Ok(Some((entry, stats)))
}

struct FileEntryInput<'a> {
    file_path: &'a str,
    blob_sha: &'a str,
    author_id: &'a str,
    is_ai_checkpoint: bool,
    previous_content: &'a str,
    previous_attributions: &'a [Attribution],
    content: &'a str,
    ts: u128,
}

fn make_entry_for_file(
    input: FileEntryInput<'_>,
) -> Result<(WorkingLogEntry, FileLineStats), GitAiError> {
    let FileEntryInput {
        file_path,
        blob_sha,
        author_id,
        is_ai_checkpoint,
        previous_content,
        previous_attributions,
        content,
        ts,
    } = input;

    let tracker = AttributionTracker::new();

    let fill_start = Instant::now();
    let filled_in_prev_attributions = tracker.attribute_unattributed_ranges(
        previous_content,
        previous_attributions,
        &CheckpointKind::Human.to_str(),
        ts - 1,
    );
    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK]   attribute_unattributed_ranges for {} took {:?}",
        file_path,
        fill_start.elapsed()
    );

    let update_start = Instant::now();
    let new_attributions = tracker.update_attributions_for_checkpoint(
        previous_content,
        content,
        &filled_in_prev_attributions,
        author_id,
        ts,
        is_ai_checkpoint,
    )?;
    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK]   update_attributions for {} took {:?}",
        file_path,
        update_start.elapsed()
    );

    // TODO Consider discarding any "uncontentious" attributions for the human author. Any human attributions that do not share a line with any other author's attributions can be discarded.
    // let filtered_attributions = crate::model::attribution_tracker::discard_uncontentious_attributions_for_author(&new_attributions, &CheckpointKind::Human.to_str());

    let line_attr_start = Instant::now();
    let line_attributions =
        crate::model::attribution_tracker::attributions_to_line_attributions_for_checkpoint(
            &new_attributions,
            content,
            is_ai_checkpoint,
        );
    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK]   attributions_to_line_attributions for {} took {:?}",
        file_path,
        line_attr_start.elapsed()
    );

    // Compute line stats while we already have both contents in memory
    let stats_start = Instant::now();
    let line_stats = compute_file_line_stats(previous_content, content);
    tracing::debug!(target: "git_ai::operations::daemon::checkpoint",
        "[BENCHMARK]   compute_file_line_stats for {} took {:?}",
        file_path,
        stats_start.elapsed()
    );

    let entry = WorkingLogEntry::new(
        file_path.to_string(),
        blob_sha.to_string(),
        new_attributions,
        line_attributions,
    );

    Ok((entry, line_stats))
}
