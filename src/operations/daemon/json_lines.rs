use crate::error::GitAiError;
use std::io::{self, BufRead, BufReader, Read};

pub const DAEMON_TRACE_MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

pub struct TraceReader<R> {
    reader: BufReader<R>,
    // Bootstrap timeouts transfer this unfinished frame to the worker. Dropping
    // it would lose evidence and reset the byte budget in the middle of a line.
    pending: Vec<u8>,
}

impl<R: Read> TraceReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            pending: Vec::new(),
        }
    }

    pub fn get_ref(&self) -> &R {
        self.reader.get_ref()
    }

    pub fn read_line(&mut self) -> Result<Option<String>, GitAiError> {
        read_bounded_line(
            &mut self.reader,
            &mut self.pending,
            DAEMON_TRACE_MAX_FRAME_BYTES,
            "trace",
        )
    }
}

pub fn read_json_line_bounded<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Option<String>, GitAiError> {
    read_bounded_line(reader, &mut Vec::new(), max_bytes, "control")
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    pending: &mut Vec<u8>,
    max_bytes: usize,
    frame_kind: &str,
) -> Result<Option<String>, GitAiError> {
    let remaining = max_bytes.saturating_add(1).saturating_sub(pending.len());
    let mut limited = reader.take(u64::try_from(remaining).unwrap_or(u64::MAX));
    limited.read_until(b'\n', pending)?;
    if pending.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("daemon {frame_kind} frame exceeds {max_bytes} bytes"),
        )
        .into());
    }
    if pending.is_empty() {
        return Ok(None);
    }
    String::from_utf8(std::mem::take(pending))
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error).into())
}

#[cfg(test)]
mod tests;
