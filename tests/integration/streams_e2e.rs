//! End-to-end integration tests for transcript processing system.
//!
//! Tests the database integration and session record management.
//! The actual transcript processing and metrics emission are tested via
//! daemon tests and manual verification.

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::{fixture_path, transcript_fixture_path};
use git_ai::metrics::{
    EventAttributes, MetricEvent, OtelTraceValues, PosEncoded, SessionEventValues,
};
use git_ai::model::stream_watermark::{
    ByteOffsetWatermark, TimestampCursorWatermark, TimestampWatermark, WatermarkStrategy,
    WatermarkType,
};
use git_ai::operations::streams::agent::Agent;
use git_ai::operations::streams::agents::{ClaudeAgent, CopilotAgent, OpenCodeAgent};
use git_ai::operations::streams::sweep::StreamFormat;
use git_ai::operations::streams::{StreamRecord, StreamsDatabase};
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

mod copilot_otel;
mod database_state;
mod session_identity;
