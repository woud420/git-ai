use super::revision::{JournalRevision, RevisionHasher};
use super::{
    CHECKPOINT_API_VERSION, CheckpointJournalError, GitAiError, JournalLocation, LoadedJournal,
    decode_record, migrate_prompt_hashes, prune_old_char_attributions, reset_if_oversized, wire_v1,
    wire_v2,
};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};

pub(super) struct ReadResult {
    pub(super) journal: LoadedJournal,
    pub(super) revision: JournalRevision,
}

pub(super) fn read_locked(
    location: &JournalLocation<'_>,
    max_bytes: u64,
) -> Result<ReadResult, GitAiError> {
    let path = location.checkpoints_file();
    if !path.exists() {
        return Ok(ReadResult {
            journal: LoadedJournal::empty(),
            revision: JournalRevision::Missing,
        });
    }
    if reset_if_oversized(location, max_bytes)? {
        return Ok(ReadResult {
            journal: LoadedJournal::empty(),
            revision: RevisionHasher::new().finish(),
        });
    }

    let file = fs::File::open(&path)?;
    let mut reader = BufReader::new(&file);
    let mut checkpoints = Vec::new();
    let mut accepted = RevisionHasher::new();
    let mut offset = 0_u64;
    let mut truncate_to = None;
    let mut terminate_legacy_tail = false;
    let mut contains_legacy_records = false;

    loop {
        let record_start = offset;
        let record_start_revision = accepted.clone();
        let mut bytes = Vec::new();
        let read = reader.read_until(b'\n', &mut bytes)?;
        if read == 0 {
            break;
        }
        offset += read as u64;
        accepted.update(&bytes);
        let terminated = bytes.last() == Some(&b'\n');
        if terminated {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        if bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
            if !terminated {
                truncate_to = Some((record_start, record_start_revision));
                break;
            }
            continue;
        }
        if !terminated && is_versioned_record(&bytes) {
            truncate_to = Some((record_start, record_start_revision));
            break;
        }

        let (checkpoint, is_versioned) = match decode_record(&bytes) {
            Ok(decoded) => decoded,
            Err(CheckpointJournalError::Json(_)) if !terminated => {
                truncate_to = Some((record_start, record_start_revision));
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
        contains_legacy_records |= !is_versioned;
        checkpoints.push(checkpoint);
    }

    drop(reader);
    if let Some((length, prefix_revision)) = truncate_to {
        let repair = OpenOptions::new().write(true).open(&path)?;
        repair.set_len(length)?;
        repair.sync_all()?;
        accepted = prefix_revision;
    } else if terminate_legacy_tail {
        let mut repair = OpenOptions::new().append(true).open(&path)?;
        repair.write_all(b"\n")?;
        repair.sync_all()?;
        accepted.update(b"\n");
    }

    migrate_prompt_hashes(&mut checkpoints);
    prune_old_char_attributions(&mut checkpoints);
    Ok(ReadResult {
        journal: LoadedJournal::from_checkpoints(checkpoints, contains_legacy_records),
        revision: accepted.finish(),
    })
}

fn is_versioned_record(bytes: &[u8]) -> bool {
    wire_v1::is_record(bytes) || wire_v2::is_record(bytes)
}
