use super::reader::read_locked;
use super::revision::{JournalRevision, RevisionHasher, RevisionSnapshot, snapshot};
use super::{
    Checkpoint, CheckpointJournalError, GitAiError, JournalLocation, LoadedJournal, LockFile,
    acquire_lock, replace_file_durably, reset_if_oversized, sync_checkpoint_blobs,
    sync_parent_directory, wire_v2,
};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const PROCESS_CACHE_CAPACITY: usize = 2;
const PROCESS_CACHE_MAX_RETAINED_BYTES: u64 = 800 * 1024;
const MAX_COLD_READ_ATTEMPTS: usize = 3;

#[cfg(test)]
thread_local! {
    static DECODE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn record_decode() {
    DECODE_COUNT.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
pub(super) fn reset_decode_count() {
    DECODE_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(super) fn decode_count() -> usize {
    DECODE_COUNT.with(std::cell::Cell::get)
}

struct CacheEntry {
    path: PathBuf,
    revision: JournalRevision,
    journal: LoadedJournal,
    weight_bytes: u64,
}

pub(super) struct JournalCache {
    capacity: usize,
    max_retained_bytes: u64,
    retained_bytes: u64,
    entries: VecDeque<CacheEntry>,
}

impl JournalCache {
    pub(super) fn new(capacity: usize, max_retained_bytes: u64) -> Self {
        Self {
            capacity,
            max_retained_bytes,
            retained_bytes: 0,
            entries: VecDeque::with_capacity(capacity),
        }
    }

    fn take(&mut self, path: &Path) -> Option<CacheEntry> {
        let position = self.entries.iter().position(|entry| entry.path == path)?;
        let entry = self.entries.remove(position)?;
        self.retained_bytes = self.retained_bytes.saturating_sub(entry.weight_bytes);
        Some(entry)
    }

    fn insert(&mut self, entry: CacheEntry) -> Vec<CacheEntry> {
        let mut evicted = Vec::new();
        if self.capacity == 0 || entry.weight_bytes > self.max_retained_bytes {
            evicted.push(entry);
            return evicted;
        }
        let target_path = &entry.path;
        if let Some(position) = self
            .entries
            .iter()
            .position(|candidate| &candidate.path == target_path)
            && let Some(removed) = self.entries.remove(position)
        {
            self.retained_bytes = self.retained_bytes.saturating_sub(removed.weight_bytes);
            evicted.push(removed);
        }
        while self.entries.len() >= self.capacity
            || self.retained_bytes.saturating_add(entry.weight_bytes) > self.max_retained_bytes
        {
            let Some(removed) = self.entries.pop_front() else {
                break;
            };
            self.retained_bytes = self.retained_bytes.saturating_sub(removed.weight_bytes);
            evicted.push(removed);
        }
        self.retained_bytes = self.retained_bytes.saturating_add(entry.weight_bytes);
        self.entries.push_back(entry);
        evicted
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

pub(super) fn retained_capacity_bytes(journal: &LoadedJournal) -> u64 {
    let mut bytes = size_of::<LoadedJournal>()
        .saturating_add(journal.checkpoints.capacity() * size_of::<Checkpoint>());
    for checkpoint in journal.iter() {
        bytes = bytes
            .saturating_add(checkpoint.diff.capacity())
            .saturating_add(checkpoint.author.capacity())
            .saturating_add(checkpoint.api_version.capacity())
            .saturating_add(
                checkpoint.entries.capacity()
                    * size_of::<crate::model::working_log::WorkingLogEntry>(),
            );
        for entry in &checkpoint.entries {
            bytes = bytes
                .saturating_add(entry.file.capacity())
                .saturating_add(entry.blob_sha.capacity())
                .saturating_add(
                    entry.attributions.capacity()
                        * size_of::<crate::model::attribution::Attribution>(),
                )
                .saturating_add(
                    entry.line_attributions.capacity()
                        * size_of::<crate::model::attribution::LineAttribution>(),
                );
            bytes = entry.attributions.iter().fold(bytes, |total, attribution| {
                total.saturating_add(attribution.author_id.capacity())
            });
            bytes = entry
                .line_attributions
                .iter()
                .fold(bytes, |total, attribution| {
                    total
                        .saturating_add(attribution.author_id.capacity())
                        .saturating_add(attribution.overrode.as_ref().map_or(0, String::capacity))
                });
        }
        if let Some(agent) = &checkpoint.agent_id {
            bytes = bytes
                .saturating_add(agent.tool.capacity())
                .saturating_add(agent.id.capacity())
                .saturating_add(agent.model.capacity());
        }
        if let Some(metadata) = &checkpoint.agent_metadata {
            bytes =
                bytes.saturating_add(metadata.capacity() * (size_of::<(String, String)>() + 16));
            for (key, value) in metadata {
                bytes = bytes
                    .saturating_add(key.capacity())
                    .saturating_add(value.capacity());
            }
        }
        for value in [
            checkpoint.git_ai_version.as_ref(),
            checkpoint.trace_id.as_ref(),
            checkpoint.delivery_id.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            bytes = bytes.saturating_add(value.capacity());
        }
        if let Some(metadata) = &checkpoint.known_human_metadata {
            bytes = bytes
                .saturating_add(metadata.editor.capacity())
                .saturating_add(metadata.editor_version.capacity())
                .saturating_add(metadata.extension_version.capacity());
        }
    }
    // Allow for allocator bookkeeping, hash-table control bytes, and alignment.
    (bytes as u64).saturating_mul(2).saturating_add(4096)
}

fn process_cache() -> &'static Mutex<JournalCache> {
    static CACHE: OnceLock<Mutex<JournalCache>> = OnceLock::new();
    // Ownership stays at the journal boundary: leases move decoded state out
    // while holding the cross-process journal lock, so no actor can clone or
    // concurrently observe cached history.
    CACHE.get_or_init(|| {
        Mutex::new(JournalCache::new(
            PROCESS_CACHE_CAPACITY,
            PROCESS_CACHE_MAX_RETAINED_BYTES,
        ))
    })
}

pub(crate) fn reset(location: &JournalLocation<'_>) -> Result<(), GitAiError> {
    let _lock = acquire_lock(location)?;
    let blobs_directory = location.blobs_directory();
    if blobs_directory.exists() {
        fs::remove_dir_all(blobs_directory)?;
    }
    super::reset_file_durably(&location.checkpoints_file())
}

fn lock_cache(cache: &Mutex<JournalCache>) -> std::sync::MutexGuard<'_, JournalCache> {
    cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn take_matching(
    cache: &Mutex<JournalCache>,
    path: &Path,
    revision: &JournalRevision,
) -> Option<LoadedJournal> {
    let entry = { lock_cache(cache).take(path) }?;
    (entry.revision == *revision).then_some(entry.journal)
}

fn store_cached(
    cache: &Mutex<JournalCache>,
    path: PathBuf,
    revision: JournalRevision,
    journal: LoadedJournal,
) {
    let weight_bytes = retained_capacity_bytes(&journal);
    let entry = CacheEntry {
        path,
        revision,
        journal,
        weight_bytes,
    };
    let evicted = { lock_cache(cache).insert(entry) };
    drop(evicted);
}

pub(super) fn read_cached(
    location: &JournalLocation<'_>,
    max_bytes: u64,
) -> Result<JournalLease<'static>, GitAiError> {
    read_cached_with_cache(location, max_bytes, process_cache())
}

pub(super) fn read_cached_with_cache<'a>(
    location: &JournalLocation<'_>,
    max_bytes: u64,
    cache: &'a Mutex<JournalCache>,
) -> Result<JournalLease<'a>, GitAiError> {
    read_cached_inner(location, max_bytes, cache, &mut || {})
}

#[cfg(test)]
pub(super) fn read_cached_with_cache_after_decode<'a>(
    location: &JournalLocation<'_>,
    max_bytes: u64,
    cache: &'a Mutex<JournalCache>,
    after_decode: &mut dyn FnMut(),
) -> Result<JournalLease<'a>, GitAiError> {
    read_cached_inner(location, max_bytes, cache, after_decode)
}

fn read_cached_inner<'a>(
    location: &JournalLocation<'_>,
    max_bytes: u64,
    cache: &'a Mutex<JournalCache>,
    after_decode: &mut dyn FnMut(),
) -> Result<JournalLease<'a>, GitAiError> {
    let lock = acquire_lock(location)?;
    let path = location.checkpoints_file();
    for _ in 0..MAX_COLD_READ_ATTEMPTS {
        if reset_if_oversized(location, max_bytes)? {
            let removed = {
                let mut cache = lock_cache(cache);
                cache.take(&path)
            };
            drop(removed);
        }
        let current = snapshot(&path)?;
        if let Some(journal) = take_matching(cache, &path, &current.revision) {
            return Ok(JournalLease::new(
                cache,
                location,
                path,
                lock,
                journal,
                current.revision,
            ));
        }
        let decoded = read_locked(location, max_bytes)?;
        after_decode();
        let final_snapshot = snapshot(&path)?;
        if decoded.revision == final_snapshot.revision {
            return Ok(JournalLease::new(
                cache,
                location,
                path,
                lock,
                decoded.journal,
                final_snapshot.revision,
            ));
        }
    }
    Err(CheckpointJournalError::Integrity(format!(
        "checkpoint journal changed repeatedly while reading: {}",
        path.display()
    ))
    .into())
}

pub(crate) struct JournalLease<'a> {
    cache: &'a Mutex<JournalCache>,
    path: PathBuf,
    blobs_directory: PathBuf,
    lock: Option<LockFile>,
    journal: Option<LoadedJournal>,
    revision: JournalRevision,
    healthy: bool,
}

impl JournalLease<'_> {
    fn new<'a>(
        cache: &'a Mutex<JournalCache>,
        location: &JournalLocation<'_>,
        path: PathBuf,
        lock: LockFile,
        journal: LoadedJournal,
        revision: JournalRevision,
    ) -> JournalLease<'a> {
        JournalLease {
            cache,
            path,
            blobs_directory: location.blobs_directory(),
            lock: Some(lock),
            journal: Some(journal),
            revision,
            healthy: true,
        }
    }

    fn conflict(&mut self) -> GitAiError {
        self.healthy = false;
        CheckpointJournalError::Integrity(format!(
            "checkpoint journal changed after checkout: {}",
            self.path.display()
        ))
        .into()
    }

    fn verify_revision(&mut self) -> Result<RevisionSnapshot, GitAiError> {
        let current = match snapshot(&self.path) {
            Ok(current) => current,
            Err(error) => {
                self.healthy = false;
                return Err(error);
            }
        };
        if current.revision != self.revision {
            return Err(self.conflict());
        }
        Ok(current)
    }

    fn fail<T>(&mut self, result: Result<T, GitAiError>) -> Result<T, GitAiError> {
        if result.is_err() {
            self.healthy = false;
        }
        result
    }

    fn finish_publication<T>(&mut self, result: Result<T, GitAiError>) -> Result<T, GitAiError> {
        self.healthy = result.is_ok();
        result
    }

    pub(crate) fn append_checkpoint(
        &mut self,
        checkpoint: Checkpoint,
        compaction_interval: usize,
    ) -> Result<(), GitAiError> {
        self.append_checkpoint_inner(checkpoint, compaction_interval, &mut || {})
    }

    #[cfg(test)]
    pub(super) fn append_checkpoint_after_mutation(
        &mut self,
        checkpoint: Checkpoint,
        compaction_interval: usize,
        after_mutation: &mut dyn FnMut(),
    ) -> Result<(), GitAiError> {
        self.append_checkpoint_inner(checkpoint, compaction_interval, after_mutation)
    }

    fn append_checkpoint_inner(
        &mut self,
        checkpoint: Checkpoint,
        compaction_interval: usize,
        after_mutation: &mut dyn FnMut(),
    ) -> Result<(), GitAiError> {
        self.healthy = false;
        let journal = self
            .journal
            .as_mut()
            .expect("checked-out journal is present");
        let rewrite = journal.contains_legacy_records()
            || (compaction_interval > 0 && (journal.len() + 1).is_multiple_of(compaction_interval));
        journal.push(checkpoint);
        super::prune_old_char_attributions(journal.as_mut_slice());
        if rewrite {
            journal.mark_rewritten();
        }
        after_mutation();
        if rewrite {
            self.rewrite()
        } else {
            self.append_last()
        }
    }

    fn append_last(&mut self) -> Result<(), GitAiError> {
        let mut snapshot = self.verify_revision()?;
        crate::observability::wltrace::record("working_log.append.begin", &self.path, String::new);
        let result = (|| {
            let checkpoint = self
                .journal
                .as_ref()
                .and_then(|journal| journal.last())
                .expect("checked-out journal contains the checkpoint being appended");
            sync_checkpoint_blobs_at(&self.blobs_directory, checkpoint, false)?;
            let mut record = wire_v2::encode(checkpoint)?;
            record.push(b'\n');
            let created = matches!(snapshot.revision, JournalRevision::Missing);
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            file.write_all(&record)?;
            file.sync_all()?;
            if created {
                sync_parent_directory(&self.path)?;
            }
            snapshot.hasher.update(&record);
            self.revision = snapshot.hasher.finish();
            Ok(())
        })();
        if result.is_ok() {
            crate::observability::wltrace::record(
                "working_log.append.end",
                &self.path,
                String::new,
            );
        }
        self.finish_publication(result)
    }

    fn rewrite(&mut self) -> Result<(), GitAiError> {
        self.verify_revision()?;
        crate::observability::wltrace::record("working_log.rewrite.begin", &self.path, || {
            format!(
                "checkpoints={}",
                self.journal.as_ref().map_or(0, |journal| journal.len())
            )
        });
        let result = (|| {
            let journal = self
                .journal
                .as_ref()
                .expect("checked-out journal is present");
            sync_checkpoint_blobs_in(&self.blobs_directory, journal.iter())?;
            let temp_path = self.path.with_extension("jsonl.tmp");
            let mut output = std::io::BufWriter::new(fs::File::create(&temp_path)?);
            let mut hasher = RevisionHasher::new();
            for checkpoint in journal.iter() {
                let record = wire_v2::encode(checkpoint)?;
                output.write_all(&record)?;
                output.write_all(b"\n")?;
                hasher.update(&record);
                hasher.update(b"\n");
            }
            output.flush()?;
            output
                .into_inner()
                .map_err(|error| error.into_error())?
                .sync_all()?;
            replace_file_durably(&temp_path, &self.path)?;
            self.revision = hasher.finish();
            Ok(())
        })();
        if result.is_ok() {
            crate::observability::wltrace::record("working_log.rewrite.end", &self.path, || {
                format!(
                    "checkpoints={}",
                    self.journal.as_ref().map_or(0, |journal| journal.len())
                )
            });
        }
        self.finish_publication(result)
    }

    pub(crate) fn ensure_durable(&mut self, checkpoint_index: usize) -> Result<(), GitAiError> {
        self.verify_revision()?;
        let result = (|| {
            let checkpoint = &self
                .journal
                .as_ref()
                .expect("checked-out journal is present")[checkpoint_index];
            sync_checkpoint_blobs_at(&self.blobs_directory, checkpoint, true)?;
            OpenOptions::new()
                .write(true)
                .open(&self.path)?
                .sync_all()?;
            sync_parent_directory(&self.path)
        })();
        self.fail(result)
    }
}

fn sync_checkpoint_blobs_at(
    blobs_directory: &Path,
    checkpoint: &Checkpoint,
    verify_contents: bool,
) -> Result<(), GitAiError> {
    sync_checkpoint_blobs_with_policy(
        blobs_directory,
        std::iter::once(checkpoint),
        verify_contents,
    )
}

fn sync_checkpoint_blobs_in<'a>(
    blobs_directory: &Path,
    checkpoints: impl IntoIterator<Item = &'a Checkpoint>,
) -> Result<(), GitAiError> {
    sync_checkpoint_blobs_with_policy(blobs_directory, checkpoints, true)
}

fn sync_checkpoint_blobs_with_policy<'a>(
    blobs_directory: &Path,
    checkpoints: impl IntoIterator<Item = &'a Checkpoint>,
    verify_contents: bool,
) -> Result<(), GitAiError> {
    let directory = blobs_directory
        .parent()
        .expect("blobs directory has a parent");
    let location = JournalLocation::new(directory, "cached");
    sync_checkpoint_blobs(&location, checkpoints, verify_contents)
}

impl Deref for JournalLease<'_> {
    type Target = LoadedJournal;

    fn deref(&self) -> &Self::Target {
        self.journal
            .as_ref()
            .expect("checked-out journal is present")
    }
}

impl Drop for JournalLease<'_> {
    fn drop(&mut self) {
        drop(self.lock.take());
        if self.healthy
            && let Some(journal) = self.journal.take()
        {
            store_cached(
                self.cache,
                self.path.clone(),
                self.revision.clone(),
                journal,
            );
        }
    }
}
