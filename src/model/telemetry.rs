//! Telemetry envelope DTO exchanged between clients and the daemon.
//!
//! `TelemetryEnvelope` is a pure serialization shape (error/performance/
//! message/metrics events) produced by observability call sites and consumed by
//! the daemon's telemetry worker. It lives in `model` so producers such as
//! `observability` do not have to reach into `operations::daemon` just to name
//! the payload type.

use crate::metrics::MetricEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// A telemetry envelope sent from client to daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TelemetryEnvelope {
    Error {
        timestamp: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<Value>,
    },
    Performance {
        timestamp: String,
        operation: String,
        duration_ms: u128,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tags: Option<HashMap<String, String>>,
    },
    Message {
        timestamp: String,
        message: String,
        level: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<Value>,
    },
    Metrics {
        events: Vec<MetricEvent>,
    },
}
