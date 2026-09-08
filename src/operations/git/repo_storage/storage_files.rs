use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub const MAX_CHECKPOINTS_JSONL_BYTES: u64 = 1024 * 1024 * 1024;

#[cfg(feature = "test-support")]
pub(super) const TEST_CHECKPOINTS_JSONL_MAX_BYTES_ENV: &str =
    "GIT_AI_TEST_CHECKPOINTS_JSONL_MAX_BYTES";

pub(super) fn persistence_error(kind: std::io::ErrorKind, message: String) -> PersistenceError {
    PersistenceError::Io {
        // Preserve the externally observed text while moving to the layered error type.
        operation: "Generic error",
        path: String::new(),
        kind,
        message,
    }
}

pub(super) fn create_directory_durably(path: &Path) -> Result<(), GitAiError> {
    #[cfg(unix)]
    let created = !path.exists();
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    if created && let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

pub(crate) fn persist_file_version_to_blob_dir(
    blobs_dir: &Path,
    content: &str,
) -> Result<String, GitAiError> {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let sha = format!("{:x}", hasher.finalize());

    create_directory_durably(blobs_dir)?;
    let blob_path = blobs_dir.join(&sha);
    match fs::read(&blob_path) {
        Ok(existing) if existing == content.as_bytes() => return Ok(sha),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::write(blob_path, content.as_bytes())?;

    Ok(sha)
}
