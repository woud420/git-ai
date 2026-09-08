//! Watermarking strategies for tracking transcript processing progress.

use crate::model::stream_types::StreamError;
use chrono::{DateTime, Utc};
use std::fmt;
use std::str::FromStr;

/// Strategy for tracking progress through a transcript.
pub trait WatermarkStrategy: Send + Sync {
    /// Serialize the watermark to a string for database storage.
    fn serialize(&self) -> String;

    /// Advance the watermark based on bytes and records read.
    fn advance(&mut self, bytes_read: usize, records_read: usize);

    /// Downcast support for concrete watermark types.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Type of watermark strategy (used for deserialization).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatermarkType {
    ByteOffset,
    RecordIndex,
    Timestamp,
    Hybrid,
    TimestampCursor,
}

impl fmt::Display for WatermarkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WatermarkType::ByteOffset => write!(f, "ByteOffset"),
            WatermarkType::RecordIndex => write!(f, "RecordIndex"),
            WatermarkType::Timestamp => write!(f, "Timestamp"),
            WatermarkType::Hybrid => write!(f, "Hybrid"),
            WatermarkType::TimestampCursor => write!(f, "TimestampCursor"),
        }
    }
}

impl FromStr for WatermarkType {
    type Err = StreamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ByteOffset" => Ok(WatermarkType::ByteOffset),
            "RecordIndex" => Ok(WatermarkType::RecordIndex),
            "Timestamp" => Ok(WatermarkType::Timestamp),
            "Hybrid" => Ok(WatermarkType::Hybrid),
            "TimestampCursor" => Ok(WatermarkType::TimestampCursor),
            _ => Err(StreamError::Parse {
                line: 0,
                message: format!("Invalid watermark type: {}", s),
            }),
        }
    }
}

impl WatermarkType {
    /// Deserialize a watermark value based on the strategy type.
    pub fn deserialize(&self, s: &str) -> Result<Box<dyn WatermarkStrategy>, StreamError> {
        match self {
            WatermarkType::ByteOffset => Ok(Box::new(ByteOffsetWatermark::from_str(s)?)),
            WatermarkType::RecordIndex => Ok(Box::new(RecordIndexWatermark::from_str(s)?)),
            WatermarkType::Timestamp => Ok(Box::new(TimestampWatermark::from_str(s)?)),
            WatermarkType::Hybrid => Ok(Box::new(HybridWatermark::from_str(s)?)),
            WatermarkType::TimestampCursor => Ok(Box::new(TimestampCursorWatermark::from_str(s)?)),
        }
    }

    pub fn create_initial_watermark(&self) -> Box<dyn WatermarkStrategy> {
        match self {
            WatermarkType::ByteOffset => Box::new(ByteOffsetWatermark::new(0)),
            WatermarkType::RecordIndex => Box::new(RecordIndexWatermark::new(0)),
            WatermarkType::Timestamp => Box::new(TimestampWatermark::new(
                chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
            )),
            WatermarkType::Hybrid => Box::new(HybridWatermark::new(0, 0, None)),
            WatermarkType::TimestampCursor => Box::new(TimestampCursorWatermark::initial()),
        }
    }
}

/// Byte-offset based watermark for append-only files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteOffsetWatermark(pub u64);

impl ByteOffsetWatermark {
    pub fn new(offset: u64) -> Self {
        Self(offset)
    }
}

impl WatermarkStrategy for ByteOffsetWatermark {
    fn serialize(&self) -> String {
        self.0.to_string()
    }

    fn advance(&mut self, bytes_read: usize, _records_read: usize) {
        self.0 += bytes_read as u64;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl FromStr for ByteOffsetWatermark {
    type Err = StreamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>()
            .map(ByteOffsetWatermark)
            .map_err(|e| StreamError::Parse {
                line: 0,
                message: format!("Invalid byte offset watermark: {}", e),
            })
    }
}

/// Record-index based watermark for sequential formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordIndexWatermark(pub u64);

impl RecordIndexWatermark {
    pub fn new(index: u64) -> Self {
        Self(index)
    }
}

impl WatermarkStrategy for RecordIndexWatermark {
    fn serialize(&self) -> String {
        self.0.to_string()
    }

    fn advance(&mut self, _bytes_read: usize, records_read: usize) {
        self.0 += records_read as u64;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl FromStr for RecordIndexWatermark {
    type Err = StreamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>()
            .map(RecordIndexWatermark)
            .map_err(|e| StreamError::Parse {
                line: 0,
                message: format!("Invalid record index watermark: {}", e),
            })
    }
}

/// Timestamp-based watermark for time-ordered streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampWatermark(pub DateTime<Utc>);

impl TimestampWatermark {
    pub fn new(timestamp: DateTime<Utc>) -> Self {
        Self(timestamp)
    }
}

impl WatermarkStrategy for TimestampWatermark {
    fn serialize(&self) -> String {
        self.0.to_rfc3339()
    }

    fn advance(&mut self, _bytes_read: usize, _records_read: usize) {
        // Timestamp watermarks don't auto-advance
        // They must be explicitly updated based on record timestamps
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl FromStr for TimestampWatermark {
    type Err = StreamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        DateTime::parse_from_rfc3339(s)
            .map(|dt| TimestampWatermark(dt.with_timezone(&Utc)))
            .map_err(|e| StreamError::Parse {
                line: 0,
                message: format!("Invalid timestamp watermark: {}", e),
            })
    }
}

/// Timestamp + cursor watermark for keyset pagination over time-ordered data.
/// Stores (timestamp_millis, last_cursor_id) to handle ties at batch boundaries.
/// The cursor is the last-seen ID at the watermark timestamp, enabling
/// `WHERE (ts > ?1 OR (ts = ?1 AND id > ?2))` style queries.
#[derive(Debug, Clone, PartialEq)]
pub struct TimestampCursorWatermark {
    pub timestamp_millis: f64,
    pub last_id: String,
}

impl TimestampCursorWatermark {
    pub fn new(timestamp_millis: f64, last_id: String) -> Self {
        Self {
            timestamp_millis,
            last_id,
        }
    }

    pub fn initial() -> Self {
        Self {
            timestamp_millis: 0.0,
            last_id: String::new(),
        }
    }
}

impl WatermarkStrategy for TimestampCursorWatermark {
    fn serialize(&self) -> String {
        format!("{}|{}", self.timestamp_millis, self.last_id)
    }

    fn advance(&mut self, _bytes_read: usize, _records_read: usize) {
        // Must be explicitly updated with new timestamp + cursor
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl FromStr for TimestampCursorWatermark {
    type Err = StreamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (ts_str, id) = s.split_once('|').ok_or_else(|| StreamError::Parse {
            line: 0,
            message: format!(
                "Invalid TimestampCursor watermark format: expected 'millis|id', got '{}'",
                s
            ),
        })?;
        let timestamp_millis = ts_str.parse::<f64>().map_err(|e| StreamError::Parse {
            line: 0,
            message: format!("Invalid timestamp in TimestampCursor watermark: {}", e),
        })?;
        Ok(Self {
            timestamp_millis,
            last_id: id.to_string(),
        })
    }
}

/// Hybrid watermark combining multiple strategies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HybridWatermark {
    pub offset: u64,
    pub record: u64,
    pub timestamp: Option<DateTime<Utc>>,
}

impl HybridWatermark {
    pub fn new(offset: u64, record: u64, timestamp: Option<DateTime<Utc>>) -> Self {
        Self {
            offset,
            record,
            timestamp,
        }
    }
}

impl WatermarkStrategy for HybridWatermark {
    fn serialize(&self) -> String {
        match &self.timestamp {
            Some(ts) => format!("{}|{}|{}", self.offset, self.record, ts.to_rfc3339()),
            None => format!("{}|{}|", self.offset, self.record),
        }
    }

    fn advance(&mut self, bytes_read: usize, records_read: usize) {
        self.offset += bytes_read as u64;
        self.record += records_read as u64;
        // Timestamp must be explicitly updated based on record data
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl FromStr for HybridWatermark {
    type Err = StreamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('|').collect();
        if parts.len() != 3 {
            return Err(StreamError::Parse {
                line: 0,
                message: format!(
                    "Invalid hybrid watermark format: expected 3 parts, got {}",
                    parts.len()
                ),
            });
        }

        let offset = parts[0].parse::<u64>().map_err(|e| StreamError::Parse {
            line: 0,
            message: format!("Invalid offset in hybrid watermark: {}", e),
        })?;

        let record = parts[1].parse::<u64>().map_err(|e| StreamError::Parse {
            line: 0,
            message: format!("Invalid record in hybrid watermark: {}", e),
        })?;

        let timestamp = if parts[2].is_empty() {
            None
        } else {
            Some(
                DateTime::parse_from_rfc3339(parts[2])
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| StreamError::Parse {
                        line: 0,
                        message: format!("Invalid timestamp in hybrid watermark: {}", e),
                    })?,
            )
        };

        Ok(HybridWatermark {
            offset,
            record,
            timestamp,
        })
    }
}

#[path = "stream_watermark_tests.rs"]
#[cfg(test)]
mod tests;
