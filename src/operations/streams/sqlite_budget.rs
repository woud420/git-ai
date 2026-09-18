use crate::config::Config;
use crate::model::stream_types::StreamError;
use std::time::Duration;

pub(super) struct SqliteReadBudget {
    remaining: u64,
    event_limit: u64,
    accepted: bool,
}

impl SqliteReadBudget {
    pub(super) fn configured() -> Self {
        Self {
            remaining: Config::get().max_transcript_batch_bytes() as u64,
            event_limit: Config::get().max_transcript_line_bytes() as u64,
            accepted: false,
        }
    }

    pub(super) fn admit(&mut self, bytes: u64, largest_event: u64) -> Result<bool, StreamError> {
        if bytes > self.remaining || largest_event > self.event_limit {
            if self.accepted {
                return Ok(false);
            }
            return Err(StreamError::Transient {
                message: "SQLite transcript event or timestamp group exceeds the read byte budget"
                    .to_string(),
                retry_after: Duration::from_secs(30),
            });
        }
        self.remaining -= bytes;
        self.accepted = true;
        Ok(true)
    }
}
