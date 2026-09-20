use super::Config;

pub const DEFAULT_MAX_TRANSCRIPT_LINE_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_TRANSCRIPT_BATCH_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_METRICS_FLUSH_CHUNK_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_TRANSCRIPT_FILE_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn positive_byte_budget(
    env_key: &str,
    file_value: Option<usize>,
    default: usize,
) -> usize {
    std::env::var(env_key)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .or(file_value.filter(|value| *value > 0))
        .unwrap_or(default)
}

impl Config {
    /// Returns the per-file size limit for checkpoint content reads.
    pub fn max_checkpoint_file_size_bytes(&self) -> usize {
        self.max_checkpoint_file_size_bytes
    }

    /// Returns the total byte budget for content in one checkpoint request.
    pub fn max_checkpoint_total_size_bytes(&self) -> usize {
        self.max_checkpoint_total_size_bytes
    }

    /// Returns the total line budget for content in one checkpoint request.
    pub fn max_checkpoint_total_lines(&self) -> usize {
        self.max_checkpoint_total_lines
    }

    pub fn max_transcript_line_bytes(&self) -> usize {
        self.max_transcript_line_bytes
    }

    pub fn max_transcript_file_bytes(&self) -> usize {
        self.max_transcript_file_bytes
    }

    pub fn max_metrics_flush_chunk_bytes(&self) -> usize {
        self.max_metrics_flush_chunk_bytes
    }

    pub fn max_transcript_batch_bytes(&self) -> usize {
        self.max_transcript_batch_bytes
    }
}
