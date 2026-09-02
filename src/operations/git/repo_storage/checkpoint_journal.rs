use crate::error::GitAiError;
use crate::model::authorship_log_serialization::generate_short_hash;
use crate::model::repository::lock_file::LockFile;
use crate::model::working_log::{CHECKPOINT_API_VERSION, Checkpoint};
use crate::operations::git::repo_storage::PersistedWorkingLog;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

mod wire_v1;
mod wire_v2;

pub(crate) const JOURNAL_RECORD_VERSION: u64 = wire_v2::VERSION;
/// Bounds stale records while keeping full rewrites off ordinary checkpoints.
pub(super) const COMPACTION_INTERVAL: usize = 256;
const VERIFIED_JOURNAL_CAPACITY: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: SystemTime,
}

// This stamp binds legacy blob validation to exact journal bytes. Checksums are
// always verified; restart or external mutation forces the legacy fence.
static VERIFIED_JOURNALS: OnceLock<Mutex<HashMap<PathBuf, FileStamp>>> = OnceLock::new();

#[derive(Debug)]
pub(crate) enum CheckpointJournalError {
    Json(serde_json::Error),
    Integrity(String),
}

impl fmt::Display for CheckpointJournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "checkpoint journal JSON error: {error}"),
            Self::Integrity(message) => {
                write!(f, "checkpoint journal integrity error: {message}")
            }
        }
    }
}

impl std::error::Error for CheckpointJournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Integrity(_) => None,
        }
    }
}

impl From<serde_json::Error> for CheckpointJournalError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<CheckpointJournalError> for GitAiError {
    fn from(error: CheckpointJournalError) -> Self {
        match error {
            CheckpointJournalError::Json(error) => Self::JsonError(error),
            CheckpointJournalError::Integrity(message) => Self::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                message,
            )),
        }
    }
}

#[cfg(test)]
pub(crate) fn decode(bytes: &[u8]) -> Result<Checkpoint, CheckpointJournalError> {
    decode_record(bytes).map(|(checkpoint, _)| checkpoint)
}

fn decode_record(bytes: &[u8]) -> Result<(Checkpoint, bool), CheckpointJournalError> {
    if let Some(checkpoint) = wire_v2::decode_if_terminal(bytes)? {
        return Ok((checkpoint, true));
    }

    let value: Value = serde_json::from_slice(bytes)?;
    if !value.is_object() {
        return Err(CheckpointJournalError::Integrity(
            "checkpoint record is not a JSON object".to_string(),
        ));
    }

    if wire_v2::claims_version(&value) {
        return wire_v2::decode(bytes).map(|checkpoint| (checkpoint, true));
    }
    if wire_v2::claims_checksum(&value) {
        return Err(CheckpointJournalError::Integrity(
            "compact journal checksum is missing a record version".to_string(),
        ));
    }
    if wire_v1::has_terminal_checksum(bytes)? {
        return wire_v1::decode(bytes).map(|checkpoint| (checkpoint, true));
    }
    if wire_v1::claims_version(&value) {
        return wire_v1::decode(bytes).map(|checkpoint| (checkpoint, true));
    }
    if wire_v1::claims_checksum(&value) {
        return Err(CheckpointJournalError::Integrity(
            "journal metadata is missing a record version".to_string(),
        ));
    }
    Ok((serde_json::from_value(value)?, false))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub(super) fn append(
    working_log: &PersistedWorkingLog,
    checkpoints: &[Checkpoint],
) -> Result<(), GitAiError> {
    let checkpoint = checkpoints
        .last()
        .expect("checkpoint collection must contain the record being appended");
    let _lock = acquire_lock(working_log)?;
    let path = working_log.checkpoints_file();
    let created = !path.exists();
    if !is_verified(&path) {
        sync_checkpoint_blobs(
            working_log,
            checkpoints
                .iter()
                .filter(|checkpoint| !checkpoint.has_journal_record_version()),
            true,
        )?;
    }
    sync_checkpoint_blobs(working_log, std::iter::once(checkpoint), false)?;
    let mut record = wire_v2::encode(checkpoint)?;
    record.push(b'\n');

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(&record)?;
    file.sync_all()?;
    if created {
        sync_parent_directory(&path)?;
    }
    mark_verified(&path);
    Ok(())
}

pub(super) fn rewrite(
    working_log: &PersistedWorkingLog,
    checkpoints: &[Checkpoint],
) -> Result<(), GitAiError> {
    let _lock = acquire_lock(working_log)?;
    let path = working_log.checkpoints_file();
    let temp_path = path.with_extension("jsonl.tmp");
    let mut output = BufWriter::new(fs::File::create(&temp_path)?);

    sync_checkpoint_blobs(working_log, checkpoints.iter(), true)?;
    for checkpoint in checkpoints {
        output.write_all(&wire_v2::encode(checkpoint)?)?;
        output.write_all(b"\n")?;
    }

    output.flush()?;
    output
        .into_inner()
        .map_err(|error| error.into_error())?
        .sync_all()?;
    replace_file_durably(&temp_path, &path)?;
    mark_verified(&path);
    Ok(())
}

pub(super) fn ensure_durable(
    working_log: &PersistedWorkingLog,
    checkpoint: &Checkpoint,
) -> Result<(), GitAiError> {
    let path = working_log.checkpoints_file();
    let _lock = acquire_lock(working_log)?;
    sync_checkpoint_blobs(working_log, std::iter::once(checkpoint), true)?;
    OpenOptions::new().write(true).open(&path)?.sync_all()?;
    sync_parent_directory(&path)?;
    Ok(())
}

pub(super) fn read(
    working_log: &PersistedWorkingLog,
    max_bytes: u64,
) -> Result<Vec<Checkpoint>, GitAiError> {
    let path = working_log.checkpoints_file();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let _lock = acquire_lock(working_log)?;
    if working_log.truncate_oversized_checkpoints_file(max_bytes)? {
        forget_verified(&path);
        return Ok(Vec::new());
    }

    let file = fs::File::open(&path)?;
    let mut reader = BufReader::new(&file);
    let mut checkpoints = Vec::new();
    let mut offset = 0_u64;
    let mut truncate_to = None;
    let mut terminate_legacy_tail = false;

    loop {
        let record_start = offset;
        let mut bytes = Vec::new();
        let read = reader.read_until(b'\n', &mut bytes)?;
        if read == 0 {
            break;
        }
        offset += read as u64;
        let terminated = bytes.last() == Some(&b'\n');
        if terminated {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        if bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
            if !terminated {
                truncate_to = Some(record_start);
                break;
            }
            continue;
        }
        if !terminated && is_versioned_record(&bytes) {
            truncate_to = Some(record_start);
            break;
        }

        let checkpoint = match decode_record(&bytes) {
            Ok((checkpoint, _)) => checkpoint,
            Err(CheckpointJournalError::Json(_)) if !terminated => {
                truncate_to = Some(record_start);
                break;
            }
            Err(error) => return Err(error.into()),
        };
        if !terminated {
            terminate_legacy_tail = true;
        }
        if checkpoint.api_version != CHECKPOINT_API_VERSION {
            tracing::debug!(
                "unsupported checkpoint api version: {} (silently skipping checkpoint)",
                checkpoint.api_version
            );
            continue;
        }
        checkpoints.push(checkpoint);
    }

    drop(reader);
    if let Some(length) = truncate_to {
        let repair = OpenOptions::new().write(true).open(&path)?;
        repair.set_len(length)?;
        repair.sync_all()?;
        forget_verified(&path);
    } else if terminate_legacy_tail {
        let mut repair = OpenOptions::new().append(true).open(&path)?;
        repair.write_all(b"\n")?;
        repair.sync_all()?;
        forget_verified(&path);
    }

    migrate_prompt_hashes(&mut checkpoints);
    working_log.prune_old_char_attributions(&mut checkpoints);
    Ok(checkpoints)
}

fn is_versioned_record(bytes: &[u8]) -> bool {
    wire_v1::is_record(bytes) || wire_v2::is_record(bytes)
}

fn verified_journals() -> &'static Mutex<HashMap<PathBuf, FileStamp>> {
    VERIFIED_JOURNALS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileStamp {
        len: metadata.len(),
        modified: metadata.modified().ok()?,
    })
}

fn is_verified(path: &Path) -> bool {
    let Some(stamp) = file_stamp(path) else {
        return false;
    };
    verified_journals()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(path)
        .is_some_and(|verified| *verified == stamp)
}

fn mark_verified(path: &Path) {
    let Some(stamp) = file_stamp(path) else {
        return;
    };
    let mut journals = verified_journals()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if journals.len() >= VERIFIED_JOURNAL_CAPACITY && !journals.contains_key(path) {
        journals.clear();
    }
    journals.insert(path.to_path_buf(), stamp);
}

fn forget_verified(path: &Path) {
    verified_journals()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(path);
}

fn sync_checkpoint_blobs<'a>(
    working_log: &PersistedWorkingLog,
    checkpoints: impl IntoIterator<Item = &'a Checkpoint>,
    verify_contents: bool,
) -> Result<(), GitAiError> {
    let blobs_dir = working_log.dir.join("blobs");
    let mut synced = HashSet::new();
    for checkpoint in checkpoints {
        for entry in &checkpoint.entries {
            if !synced.insert(entry.blob_sha.as_str()) {
                continue;
            }
            let blob_path = blobs_dir.join(&entry.blob_sha);
            if verify_contents {
                let bytes = fs::read(&blob_path)?;
                if sha256_hex(&bytes) != entry.blob_sha {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("checkpoint blob hash mismatch for {}", entry.blob_sha),
                    )
                    .into());
                }
            }
            OpenOptions::new()
                .write(true)
                .open(&blob_path)?
                .sync_all()?;
        }
    }
    if !synced.is_empty() {
        sync_directory(&blobs_dir)?;
    }
    Ok(())
}

fn migrate_prompt_hashes(checkpoints: &mut [Checkpoint]) {
    let mut old_to_new_hash: HashMap<String, String> = HashMap::new();
    for checkpoint in checkpoints.iter() {
        if let Some(agent_id) = &checkpoint.agent_id {
            let new_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
            old_to_new_hash.insert(new_hash[..7].to_string(), new_hash);
        }
    }

    for checkpoint in checkpoints {
        for entry in &mut checkpoint.entries {
            for attribution in &mut entry.attributions {
                if attribution.author_id.len() == 7
                    && let Some(new_hash) = old_to_new_hash.get(&attribution.author_id)
                {
                    attribution.author_id = new_hash.clone();
                }
            }
            for attribution in &mut entry.line_attributions {
                if attribution.author_id.len() == 7
                    && let Some(new_hash) = old_to_new_hash.get(&attribution.author_id)
                {
                    attribution.author_id = new_hash.clone();
                }
                if let Some(overrode) = &attribution.overrode
                    && overrode.len() == 7
                    && let Some(new_hash) = old_to_new_hash.get(overrode)
                {
                    attribution.overrode = Some(new_hash.clone());
                }
            }
        }
    }
}

fn acquire_lock(working_log: &PersistedWorkingLog) -> Result<LockFile, GitAiError> {
    let path = working_log.dir.join(".checkpoint-journal.lock");
    let started = Instant::now();
    loop {
        if let Some(lock) = LockFile::try_acquire(&path) {
            return Ok(lock);
        }
        if started.elapsed() >= Duration::from_secs(5) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                format!(
                    "timed out acquiring checkpoint journal lock {}",
                    path.display()
                ),
            )
            .into());
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn sync_parent_directory(path: &Path) -> Result<(), GitAiError> {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(super) fn reset_file_durably(path: &Path) -> Result<(), GitAiError> {
    let temp_path = path.with_extension("jsonl.reset.tmp");
    fs::File::create(&temp_path)?.sync_all()?;
    replace_file_durably(&temp_path, path)
}

#[cfg(not(windows))]
fn replace_file_durably(temp_path: &Path, path: &Path) -> Result<(), GitAiError> {
    fs::rename(temp_path, path)?;
    sync_parent_directory(path)
}

#[cfg(windows)]
fn replace_file_durably(temp_path: &Path, path: &Path) -> Result<(), GitAiError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    fn wide_path(path: &Path) -> Result<Vec<u16>, GitAiError> {
        let mut encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if encoded.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "checkpoint journal path contains an interior NUL",
            )
            .into());
        }
        encoded.push(0);
        Ok(encoded)
    }

    let source = wide_path(temp_path)?;
    let target = wide_path(path)?;
    // Same-directory replacement remains atomic; WRITE_THROUGH supplies the
    // metadata durability fence that a directory fsync supplies on Unix.
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), GitAiError> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests;
