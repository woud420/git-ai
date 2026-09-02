use super::GitAiError;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;

const HASH_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum JournalRevision {
    Missing,
    Present { len: u64, sha256: [u8; 32] },
}

#[derive(Clone)]
pub(super) struct RevisionHasher {
    hasher: Sha256,
    len: u64,
}

impl RevisionHasher {
    pub(super) fn new() -> Self {
        Self {
            hasher: Sha256::new(),
            len: 0,
        }
    }

    pub(super) fn update(&mut self, bytes: &[u8]) {
        self.hasher.update(bytes);
        self.len = self.len.saturating_add(bytes.len() as u64);
    }

    pub(super) fn finish(self) -> JournalRevision {
        JournalRevision::Present {
            len: self.len,
            sha256: self.hasher.finalize().into(),
        }
    }
}

pub(super) struct RevisionSnapshot {
    pub(super) revision: JournalRevision,
    pub(super) hasher: RevisionHasher,
}

pub(super) fn snapshot(path: &Path) -> Result<RevisionSnapshot, GitAiError> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RevisionSnapshot {
                revision: JournalRevision::Missing,
                hasher: RevisionHasher::new(),
            });
        }
        Err(error) => return Err(error.into()),
    };
    let mut hasher = RevisionHasher::new();
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let revision = hasher.clone().finish();
    Ok(RevisionSnapshot { revision, hasher })
}
