use super::checkpoint_journal::{self, LoadedJournal};
#[cfg(feature = "test-support")]
use super::storage_files::TEST_CHECKPOINTS_JSONL_MAX_BYTES_ENV;
use super::storage_files::{
    MAX_CHECKPOINTS_JSONL_BYTES, persist_file_version_to_blob_dir, persistence_error,
};
use crate::error::GitAiError;
use crate::model::attribution_tracker::LineAttribution;
use crate::model::authorship_log::{HumanRecord, PromptRecord, SessionRecord};
use crate::model::working_log::InitialAttributions;
use crate::model::working_log::{Checkpoint, CheckpointKind};
use crate::operations::git::path_format::normalize_to_posix;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct PersistedWorkingLog {
    pub dir: PathBuf,
    #[allow(dead_code)]
    pub base_commit: String,
    pub repo_workdir: PathBuf,
    /// Canonical (absolute, resolved) version of workdir for reliable path comparisons
    /// On Windows, this uses the \\?\ UNC prefix format
    #[allow(dead_code)]
    pub canonical_workdir: PathBuf,
    pub dirty_files: Option<HashMap<String, Arc<str>>>,
    pub initial_file: PathBuf,
}

impl PersistedWorkingLog {
    pub fn new(
        dir: PathBuf,
        base_commit: &str,
        repo_root: PathBuf,
        canonical_workdir: PathBuf,
        dirty_files: Option<HashMap<String, Arc<str>>>,
    ) -> Self {
        let initial_file = dir.join("INITIAL");
        Self {
            dir,
            base_commit: base_commit.to_string(),
            repo_workdir: repo_root,
            canonical_workdir,
            dirty_files,
            initial_file,
        }
    }

    pub fn set_dirty_files(&mut self, dirty_files: Option<HashMap<String, Arc<str>>>) {
        let normalized_dirty_files = dirty_files.map(|map| {
            map.into_iter()
                .map(|(file_path, content)| {
                    let relative_path = self.to_repo_relative_path(&file_path);
                    let normalized_path = normalize_to_posix(&relative_path);
                    (normalized_path, content)
                })
                .collect::<HashMap<_, _>>()
        });

        self.dirty_files = normalized_dirty_files;
    }

    pub fn reset_working_log(&self) -> Result<(), GitAiError> {
        checkpoint_journal::reset(&self.checkpoint_journal_location())?;
        crate::observability::wltrace::record("working_log.reset", &self.dir, String::new);

        // Clear INITIAL attributions file so stale attributions from a
        // previous working state do not persist across resets
        if self.initial_file.exists() {
            fs::remove_file(&self.initial_file)?;
        }

        Ok(())
    }

    pub fn checkpoints_file(&self) -> PathBuf {
        self.dir.join("checkpoints.jsonl")
    }

    fn checkpoint_journal_location(&self) -> checkpoint_journal::JournalLocation<'_> {
        checkpoint_journal::JournalLocation::new(&self.dir, &self.base_commit)
    }

    /* blob storage */
    pub fn get_file_version(&self, sha: &str) -> Result<String, GitAiError> {
        let blob_path = self.dir.join("blobs").join(sha);
        Ok(fs::read_to_string(blob_path)?)
    }

    #[allow(dead_code)]
    pub fn persist_file_version(&self, content: &str) -> Result<String, GitAiError> {
        let blobs_dir = self.dir.join("blobs");
        persist_file_version_to_blob_dir(&blobs_dir, content)
    }

    pub fn to_repo_absolute_path(&self, file_path: &str) -> String {
        if Path::new(file_path).is_absolute() {
            return file_path.to_string();
        }
        self.repo_workdir
            .join(file_path)
            .to_string_lossy()
            .to_string()
    }

    pub fn to_repo_relative_path(&self, file_path: &str) -> String {
        if !Path::new(file_path).is_absolute() {
            return file_path.to_string();
        }
        let path = Path::new(file_path);

        // Try without canonicalizing first
        if path.starts_with(&self.repo_workdir) {
            return path
                .strip_prefix(&self.repo_workdir)
                .unwrap()
                .to_string_lossy()
                .to_string();
        }

        // If we couldn't match yet, try canonicalizing both repo_workdir and the input path
        // On Windows, this uses the canonical_workdir that was pre-computed
        #[cfg(windows)]
        let canonical_workdir = &self.canonical_workdir;

        #[cfg(not(windows))]
        let canonical_workdir =
            crate::operations::git::canonicalize::canonicalize_or_self(&self.repo_workdir);

        let canonical_path = crate::operations::git::canonicalize::canonicalize_or_self(path);

        #[cfg(windows)]
        if canonical_path.starts_with(canonical_workdir) {
            return canonical_path
                .strip_prefix(canonical_workdir)
                .unwrap()
                .to_string_lossy()
                .to_string();
        }

        #[cfg(not(windows))]
        if canonical_path.starts_with(&canonical_workdir) {
            return canonical_path
                .strip_prefix(&canonical_workdir)
                .unwrap()
                .to_string_lossy()
                .to_string();
        }

        file_path.to_string()
    }

    pub fn read_current_file_content(&self, file_path: &str) -> Result<Arc<str>, GitAiError> {
        if let Some(ref dirty_files) = self.dirty_files
            && let Some(content) = dirty_files.get(&file_path.to_string())
        {
            return Ok(content.clone());
        }

        Err(persistence_error(
            std::io::ErrorKind::NotFound,
            format!(
                "read_current_file_content: file '{}' not found in dirty_files snapshot (filesystem fallback is not allowed in checkpoint flow)",
                file_path
            ),
        )
        .into())
    }

    /* append checkpoint */
    pub fn append_checkpoint(&self, checkpoint: &Checkpoint) -> Result<(), GitAiError> {
        let mut checkpoints = self.read_all_checkpoints()?;
        self.append_checkpoint_to(&mut checkpoints, checkpoint.clone())
    }

    /// Appends to an already-materialized checkpoint collection and persists
    /// the result. Callers that just read the working log (the daemon
    /// checkpoint path) use this to avoid re-reading and re-parsing every
    /// checkpoint a second time per append.
    pub fn append_checkpoint_to(
        &self,
        checkpoints: &mut Vec<Checkpoint>,
        checkpoint: Checkpoint,
    ) -> Result<(), GitAiError> {
        checkpoints.push(checkpoint);

        // Prune char-level attributions from older checkpoints for the same files
        // Only the most recent checkpoint per file needs char-level precision
        checkpoint_journal::prune_old_char_attributions(checkpoints);

        self.write_all_checkpoints(checkpoints)
    }

    /// Append one checksummed checkpoint index record. The daemon uses this
    /// after materializing the collection once so ordinary checkpoints do not
    /// rewrite the entire journal.
    #[cfg(test)]
    pub(crate) fn append_checkpoint_record_to(
        &self,
        journal: &mut LoadedJournal,
        checkpoint: Checkpoint,
    ) -> Result<(), GitAiError> {
        self.append_checkpoint_record_with_compaction_interval(
            journal,
            checkpoint,
            checkpoint_journal::COMPACTION_INTERVAL,
        )
    }

    pub(crate) fn append_cached_checkpoint_record_to(
        &self,
        journal: &mut checkpoint_journal::JournalLease<'_>,
        checkpoint: Checkpoint,
    ) -> Result<(), GitAiError> {
        journal.append_checkpoint(checkpoint, checkpoint_journal::COMPACTION_INTERVAL)
    }

    #[cfg(feature = "test-support")]
    pub fn append_checkpoint_record_with_compaction_interval_for_test(
        &self,
        checkpoints: &mut Vec<Checkpoint>,
        checkpoint: Checkpoint,
        compaction_interval: usize,
    ) -> Result<(), GitAiError> {
        let contains_legacy_records = self
            .load_checkpoint_journal_with_size_limit(Self::checkpoints_file_size_limit_bytes())?
            .contains_legacy_records();
        let mut journal =
            LoadedJournal::from_checkpoints(std::mem::take(checkpoints), contains_legacy_records);
        let result = self.append_checkpoint_record_with_compaction_interval(
            &mut journal,
            checkpoint,
            compaction_interval,
        );
        *checkpoints = journal.into_checkpoints();
        result
    }

    #[cfg(any(test, feature = "test-support"))]
    fn append_checkpoint_record_with_compaction_interval(
        &self,
        journal: &mut LoadedJournal,
        checkpoint: Checkpoint,
        compaction_interval: usize,
    ) -> Result<(), GitAiError> {
        let contains_legacy_records = journal.contains_legacy_records();
        journal.push(checkpoint);
        checkpoint_journal::prune_old_char_attributions(journal.as_mut_slice());

        if contains_legacy_records
            || (compaction_interval > 0 && journal.len().is_multiple_of(compaction_interval))
        {
            checkpoint_journal::rewrite(&self.checkpoint_journal_location(), journal)?;
            journal.mark_rewritten();
            return Ok(());
        }

        let checkpoint = journal
            .last()
            .expect("checkpoint collection must contain the record being appended");
        checkpoint_journal::append(&self.checkpoint_journal_location(), checkpoint)
    }

    pub(crate) fn ensure_cached_checkpoint_record_durable(
        &self,
        journal: &mut checkpoint_journal::JournalLease<'_>,
        checkpoint_index: usize,
    ) -> Result<(), GitAiError> {
        journal.ensure_durable(checkpoint_index)
    }

    pub fn read_all_checkpoints(&self) -> Result<Vec<Checkpoint>, GitAiError> {
        self.load_checkpoint_journal_with_size_limit(Self::checkpoints_file_size_limit_bytes())
            .map(LoadedJournal::into_checkpoints)
    }

    #[cfg(test)]
    pub(crate) fn load_checkpoint_journal(&self) -> Result<LoadedJournal, GitAiError> {
        self.load_checkpoint_journal_with_size_limit(Self::checkpoints_file_size_limit_bytes())
    }

    pub(crate) fn load_cached_checkpoint_journal(
        &self,
    ) -> Result<checkpoint_journal::JournalLease<'static>, GitAiError> {
        checkpoint_journal::read_cached(
            &self.checkpoint_journal_location(),
            Self::checkpoints_file_size_limit_bytes(),
        )
    }

    #[cfg(feature = "test-support")]
    pub fn read_all_checkpoints_with_size_limit_for_test(
        &self,
        max_bytes: u64,
    ) -> Result<Vec<Checkpoint>, GitAiError> {
        self.load_checkpoint_journal_with_size_limit(max_bytes)
            .map(LoadedJournal::into_checkpoints)
    }

    pub fn ensure_checkpoints_file_size_limit(&self) -> Result<(), GitAiError> {
        self.read_all_checkpoints()?;
        Ok(())
    }

    fn load_checkpoint_journal_with_size_limit(
        &self,
        max_bytes: u64,
    ) -> Result<LoadedJournal, GitAiError> {
        checkpoint_journal::read(&self.checkpoint_journal_location(), max_bytes)
    }

    fn checkpoints_file_size_limit_bytes() -> u64 {
        #[cfg(feature = "test-support")]
        if let Ok(raw) = std::env::var(TEST_CHECKPOINTS_JSONL_MAX_BYTES_ENV)
            && let Ok(value) = raw.parse::<u64>()
            && value > 0
        {
            return value;
        }

        MAX_CHECKPOINTS_JSONL_BYTES
    }

    /// Write all checkpoints to the JSONL file, replacing any existing content
    /// Note: Unlike append_checkpoint(), this preserves transcripts because it's used
    /// by post-commit after transcripts have been refetched and need to be preserved
    /// for from_just_working_log() to read them.
    /// Rewrites the checkpoints file atomically: same-directory temp file,
    /// fsync, rename over the target, then directory fsync. A crash leaves
    /// either the old complete list or the new complete list — never a torn
    /// file. Outbox replay depends on this: its delivery-id dedup reads the
    /// list, so a torn write could otherwise both lose the recorded id and
    /// let the replay apply the delivery twice.
    pub fn write_all_checkpoints(&self, checkpoints: &[Checkpoint]) -> Result<(), GitAiError> {
        checkpoint_journal::rewrite(&self.checkpoint_journal_location(), checkpoints)
    }

    pub fn mutate_all_checkpoints<F>(&self, mutator: F) -> Result<Vec<Checkpoint>, GitAiError>
    where
        F: FnOnce(&mut Vec<Checkpoint>) -> Result<(), GitAiError>,
    {
        let mut checkpoints = self.read_all_checkpoints()?;
        mutator(&mut checkpoints)?;
        self.write_all_checkpoints(&checkpoints)?;
        Ok(checkpoints)
    }

    pub fn all_touched_files(&self) -> Result<HashSet<String>, GitAiError> {
        let checkpoints = self.read_all_checkpoints()?;
        let mut touched_files = HashSet::new();
        for checkpoint in checkpoints {
            for entry in checkpoint.entries {
                touched_files.insert(entry.file);
            }
        }
        Ok(touched_files)
    }

    pub fn observed_file_snapshot(&self) -> Result<HashMap<String, String>, GitAiError> {
        let initial = self.read_initial_attributions();
        let mut snapshot = HashMap::new();

        for file_path in initial.files.keys() {
            let content = self
                .stored_initial_file_content_from(&initial, file_path)
                .ok_or_else(|| {
                    persistence_error(
                        std::io::ErrorKind::NotFound,
                        format!("INITIAL missing persisted file snapshot for {}", file_path),
                    )
                })?;
            snapshot.insert(file_path.clone(), content);
        }

        for checkpoint in self.read_all_checkpoints()? {
            for entry in checkpoint.entries {
                let content = self.get_file_version(&entry.blob_sha)?;
                snapshot.insert(entry.file, content);
            }
        }

        Ok(snapshot)
    }

    #[allow(dead_code)]
    pub fn all_ai_touched_files(&self) -> Result<HashSet<String>, GitAiError> {
        let checkpoints = self.read_all_checkpoints()?;
        let mut touched_files = HashSet::new();
        for checkpoint in checkpoints {
            // Only include files from AI checkpoints (AiAgent or AiTab)
            match checkpoint.kind {
                CheckpointKind::AiAgent | CheckpointKind::AiTab => {
                    for entry in checkpoint.entries {
                        touched_files.insert(entry.file);
                    }
                }
                CheckpointKind::Human | CheckpointKind::KnownHuman => {
                    // Skip human checkpoints
                }
            }
        }
        Ok(touched_files)
    }

    /* INITIAL attributions file */

    /// Persist INITIAL attributions plus exact file snapshots for the target working log.
    pub fn write_initial_attributions_with_contents(
        &self,
        attributions: HashMap<String, Vec<LineAttribution>>,
        prompts: HashMap<String, PromptRecord>,
        humans: std::collections::BTreeMap<String, HumanRecord>,
        file_contents: HashMap<String, String>,
        sessions: std::collections::BTreeMap<String, SessionRecord>,
    ) -> Result<(), GitAiError> {
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

        self.write_initial(InitialAttributions {
            files: filtered,
            prompts,
            file_blobs,
            humans,
            sessions,
        })
    }

    /// Write a fully-formed INITIAL state, preserving any persisted blob references.
    pub fn write_initial(&self, initial: InitialAttributions) -> Result<(), GitAiError> {
        let filtered_files: HashMap<String, Vec<LineAttribution>> = initial
            .files
            .into_iter()
            .filter(|(_, attrs)| !attrs.is_empty())
            .collect();

        if filtered_files.is_empty() {
            if self.initial_file.exists() {
                fs::remove_file(&self.initial_file)?;
            }
            return Ok(());
        }

        let mut file_blobs = initial.file_blobs;
        file_blobs.retain(|file_path, _| filtered_files.contains_key(file_path));

        let initial_data = InitialAttributions {
            files: filtered_files,
            prompts: initial.prompts,
            file_blobs,
            humans: initial.humans,
            sessions: initial.sessions,
        };

        let json = serde_json::to_string_pretty(&initial_data)?;
        fs::write(&self.initial_file, json)?;
        crate::observability::wltrace::record("working_log.write_initial", &self.dir, String::new);

        Ok(())
    }

    pub fn initial_file_content_from(
        &self,
        initial: &InitialAttributions,
        file_path: &str,
    ) -> Result<Option<String>, GitAiError> {
        if let Some(content) = self.stored_initial_file_content_from(initial, file_path) {
            return Ok(Some(content));
        }
        if initial.files.contains_key(file_path) {
            return Err(persistence_error(
                std::io::ErrorKind::NotFound,
                format!("INITIAL missing persisted file snapshot for {}", file_path),
            )
            .into());
        }
        Ok(None)
    }

    pub fn stored_initial_file_content_from(
        &self,
        initial: &InitialAttributions,
        file_path: &str,
    ) -> Option<String> {
        if let Some(blob_sha) = initial.file_blobs.get(file_path) {
            return self.get_file_version(blob_sha).ok();
        }
        None
    }

    pub fn latest_checkpoint_file_content(&self, file_path: &str) -> Option<String> {
        let checkpoints = self.read_all_checkpoints().ok()?;
        let entry = checkpoints.iter().rev().find_map(|checkpoint| {
            checkpoint
                .entries
                .iter()
                .find(|entry| entry.file == file_path)
        })?;
        self.get_file_version(&entry.blob_sha).ok()
    }

    pub fn effective_tracked_file_content(
        &self,
        initial: &InitialAttributions,
        file_path: &str,
    ) -> Result<Option<String>, GitAiError> {
        if let Some(content) = self.latest_checkpoint_file_content(file_path) {
            return Ok(Some(content));
        }
        self.initial_file_content_from(initial, file_path)
    }

    /// Read initial attributions from the INITIAL file.
    /// Returns empty attributions and prompts if the file doesn't exist.
    pub fn read_initial_attributions(&self) -> InitialAttributions {
        if !self.initial_file.exists() {
            return InitialAttributions::default();
        }

        match fs::read_to_string(&self.initial_file) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(initial_data) => initial_data,
                Err(e) => {
                    tracing::debug!(target: "git_ai::operations::git::repo_storage", "Failed to parse INITIAL file: {}. Returning empty.", e);
                    InitialAttributions::default()
                }
            },
            Err(e) => {
                tracing::debug!(target: "git_ai::operations::git::repo_storage", "Failed to read INITIAL file: {}. Returning empty.", e);
                InitialAttributions::default()
            }
        }
    }
}
