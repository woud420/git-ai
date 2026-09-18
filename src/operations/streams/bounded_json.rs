use crate::config::Config;
use crate::model::stream_types::StreamError;
use serde::de::DeserializeOwned;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::Duration;

#[cfg(test)]
mod tests;

pub(crate) fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, StreamError> {
    let file = File::open(path)
        .map_err(|error| super::reader::transcript_open_error(path, error, "read"))?;
    let limit = Config::get().max_transcript_file_bytes() as u64;
    let metadata = file.metadata().map_err(|error| StreamError::Transient {
        message: format!(
            "Failed to read transcript metadata for {}: {error}",
            path.display()
        ),
        retry_after: Duration::from_secs(5),
    })?;
    if metadata.len() > limit {
        return Err(over_budget(path, limit));
    }
    parse_bounded(file, path, limit)
}

fn parse_bounded<T: DeserializeOwned>(
    reader: impl Read,
    path: &Path,
    limit: u64,
) -> Result<T, StreamError> {
    // Metadata can race with an append or replacement. Bound actual reads too,
    // retaining one sentinel byte to distinguish an exact fit from truncation.
    let mut input = reader.take(limit.saturating_add(1));
    let parsed = serde_json::from_reader(BufReader::new(&mut input));
    if input.limit() == 0 {
        return Err(over_budget(path, limit));
    }
    parsed.map_err(|error| StreamError::Parse {
        line: 0,
        message: format!("Invalid JSON in {}: {error}", path.display()),
    })
}

fn over_budget(path: &Path, limit: u64) -> StreamError {
    StreamError::Transient {
        message: format!(
            "Transcript file exceeds the {limit} byte read budget: {}",
            path.display()
        ),
        retry_after: Duration::from_secs(30),
    }
}
