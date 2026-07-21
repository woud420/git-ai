// src/streams/sweep.rs

use std::path::PathBuf;
use std::time::Duration;

/// Strategy for discovering new/updated sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepStrategy {
    /// Periodic polling at the given interval
    Periodic(Duration),
    /// File system watcher (not implemented yet)
    FsWatcher,
    /// HTTP API polling (not implemented yet)
    HttpApi,
    /// No sweep support for this agent
    None,
}

/// A session discovered during a sweep.
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub session_id: String,
    pub tool: String,
    pub stream_path: PathBuf,
    pub external_session_id: String,
    pub external_parent_session_id: Option<String>,
}

pub use crate::model::stream_types::StreamFormat;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::stream_watermark::WatermarkType;

    /// Every StreamFormat variant must round-trip through Display → FromStr without loss.
    /// Also verifies Debug == Display so the old `format!("{:?}", ...)` writers were
    /// always byte-compatible with the FromStr readers.
    #[test]
    fn stream_format_display_fromstr_roundtrip() {
        let variants = [
            StreamFormat::ClaudeJsonl,
            StreamFormat::CursorJsonl,
            StreamFormat::DroidJsonl,
            StreamFormat::CopilotSessionJson,
            StreamFormat::CopilotEventStreamJsonl,
            StreamFormat::GeminiJsonl,
            StreamFormat::ContinueJson,
            StreamFormat::WindsurfJsonl,
            StreamFormat::CodexJsonl,
            StreamFormat::AmpThreadJson,
            StreamFormat::OpenCodeSqlite,
            StreamFormat::PiJsonl,
            StreamFormat::CopilotOtelSqlite,
        ];
        for variant in variants {
            let display = variant.to_string();
            let debug = format!("{:?}", variant);
            assert_eq!(
                display, debug,
                "StreamFormat::{variant:?}: Display != Debug"
            );
            let roundtrip = display
                .parse::<StreamFormat>()
                .unwrap_or_else(|_| panic!("StreamFormat::from_str failed for {display:?}"));
            assert_eq!(variant, roundtrip, "round-trip failed for {display:?}");
        }
    }

    /// Every WatermarkType variant must round-trip through Display → FromStr without loss.
    /// Also verifies Debug == Display.
    #[test]
    fn watermark_type_display_fromstr_roundtrip() {
        let variants = [
            WatermarkType::ByteOffset,
            WatermarkType::RecordIndex,
            WatermarkType::Timestamp,
            WatermarkType::Hybrid,
            WatermarkType::TimestampCursor,
        ];
        for variant in variants {
            let display = variant.to_string();
            let debug = format!("{:?}", variant);
            assert_eq!(
                display, debug,
                "WatermarkType::{variant:?}: Display != Debug"
            );
            let roundtrip = display
                .parse::<WatermarkType>()
                .unwrap_or_else(|_| panic!("WatermarkType::from_str failed for {display:?}"));
            assert_eq!(variant, roundtrip, "round-trip failed for {display:?}");
        }
    }
}
