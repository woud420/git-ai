use super::transcript_open_error;
use crate::config::Config;
use crate::model::stream_types::{JsonlLineState, StreamBatch, StreamError, read_jsonl_line};
use crate::model::stream_watermark::{ByteOffsetWatermark, WatermarkStrategy};
use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::path::Path;

pub(crate) fn read_jsonl_byte_stream(
    path: &Path,
    watermark: Box<dyn WatermarkStrategy>,
    session_id: &str,
    batch_limit: usize,
    reader_name: &str,
    open_error_verb: &str,
) -> Result<StreamBatch, StreamError> {
    let start_offset = watermark
        .as_any()
        .downcast_ref::<ByteOffsetWatermark>()
        .ok_or_else(|| StreamError::Fatal {
            message: format!(
                "{} reader requires ByteOffsetWatermark, got incompatible type for session {}",
                reader_name, session_id,
            ),
        })?
        .0;
    let (events, offset) =
        read_jsonl_event_batch(path, start_offset, batch_limit, open_error_verb, |_| true)?;
    Ok(StreamBatch {
        events,
        new_watermark: Box::new(ByteOffsetWatermark::new(offset)),
    })
}

pub(crate) fn read_jsonl_event_batch(
    path: &Path,
    start_offset: u64,
    batch_limit: usize,
    open_error_verb: &str,
    accept: impl Fn(&serde_json::Value) -> bool,
) -> Result<(Vec<serde_json::Value>, u64), StreamError> {
    let config = Config::get();
    let file =
        File::open(path).map_err(|error| transcript_open_error(path, error, open_error_verb))?;
    let mut reader = BufReader::new(file);
    reader
        .seek(SeekFrom::Start(start_offset))
        .map_err(|error| StreamError::Transient {
            message: format!("Failed to seek to offset {}: {}", start_offset, error),
            retry_after: std::time::Duration::from_secs(5),
        })?;
    let mut events = Vec::with_capacity(batch_limit);
    let mut current_offset = start_offset;
    let mut remaining = config.max_transcript_batch_bytes();
    let mut line_number = 0;
    let mut line = String::new();
    while remaining > 0 {
        let limit = config.max_transcript_line_bytes().min(remaining);
        let state = read_jsonl_line(&mut reader, &mut line, limit).map_err(|error| {
            StreamError::Transient {
                message: format!("I/O error reading line: {}", error),
                retry_after: std::time::Duration::from_secs(5),
            }
        })?;
        match state {
            JsonlLineState::Eof | JsonlLineState::Partial => break,
            JsonlLineState::Oversized => {
                // Return already-read records first; leave this record retryable.
                if !events.is_empty() {
                    break;
                }
                return Err(StreamError::Transient {
                    message: format!(
                        "Transcript record at offset {} exceeds the {} byte read budget: {}",
                        start_offset,
                        limit,
                        path.display()
                    ),
                    retry_after: std::time::Duration::from_secs(30),
                });
            }
            JsonlLineState::Complete(bytes_read) => {
                current_offset += bytes_read as u64;
                line_number += 1;
            }
        }
        if line.trim().is_empty() {
            continue;
        }
        let entry = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(line = line_number, path = %path.display(), error = %error, "skipping malformed JSON line");
                continue;
            }
        };
        // Empty batches end the worker drain, so discarded records must not consume event space.
        if accept(&entry) {
            remaining -= line.len();
            events.push(entry);
        }
        if events.len() >= batch_limit {
            break;
        }
    }
    Ok((events, current_offset))
}

/// Metadata probes stop at a bounded prefix, including an incomplete last line.
pub(crate) fn read_leading_jsonl_lines(
    path: &Path,
    max_lines: usize,
) -> std::io::Result<impl Iterator<Item = String>> {
    let mut reader = BufReader::new(File::open(path)?);
    let config = Config::get();
    let mut remaining = config.max_transcript_batch_bytes();
    let mut done = false;
    Ok(std::iter::from_fn(move || {
        if done || remaining == 0 {
            return None;
        }
        let mut line = String::new();
        match read_jsonl_line(
            &mut reader,
            &mut line,
            config.max_transcript_line_bytes().min(remaining),
        ) {
            Ok(JsonlLineState::Complete(_)) => {}
            Ok(JsonlLineState::Partial) => done = true,
            _ => {
                done = true;
                return None;
            }
        }
        remaining -= line.len();
        Some(line)
    })
    .take(max_lines))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn metadata_probe_stops_at_an_oversized_prefix() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let limit = Config::get().max_transcript_line_bytes();
        file.write_all(&vec![b' '; limit + 1]).unwrap();
        file.write_all(b"\n{\"cwd\":\"/later\"}\n").unwrap();
        assert!(
            read_leading_jsonl_lines(file.path(), 50)
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn metadata_probe_accepts_a_bounded_unterminated_last_line() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "{\"cwd\":\"/current\"}").unwrap();
        let lines: Vec<_> = read_leading_jsonl_lines(file.path(), 50).unwrap().collect();
        assert_eq!(lines, ["{\"cwd\":\"/current\"}"]);
    }
}
