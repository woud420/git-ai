//! In-memory buffer for telemetry events awaiting the next flush cycle.

use crate::metrics::MetricEvent;
use crate::model::api_types::DaemonLogEvent;
use crate::model::telemetry::TelemetryEnvelope;
use crate::operations::daemon::control_api::CasSyncPayload;
use serde_json::Value;

pub(super) const MAX_DAEMON_LOG_BUFFER_EVENTS: usize = 5000;

pub(super) struct ErrorEvent {
    pub(super) timestamp: String,
    pub(super) message: String,
    pub(super) context: Option<Value>,
}

pub(super) struct PerformanceEvent {
    pub(super) timestamp: String,
    pub(super) operation: String,
    pub(super) duration_ms: u128,
    pub(super) context: Option<Value>,
    pub(super) tags: Option<std::collections::HashMap<String, String>>,
}

pub(super) struct MessageEvent {
    pub(super) timestamp: String,
    pub(super) message: String,
    pub(super) level: String,
    pub(super) context: Option<Value>,
}

/// Accumulated telemetry events waiting to be flushed.
pub(super) struct TelemetryBuffer {
    pub(super) errors: Vec<ErrorEvent>,
    pub(super) performances: Vec<PerformanceEvent>,
    pub(super) messages: Vec<MessageEvent>,
    pub(super) metrics: Vec<MetricEvent>,
    pub(super) cas_records: Vec<CasSyncPayload>,
    pub(super) daemon_logs: Vec<DaemonLogEvent>,
}

impl TelemetryBuffer {
    pub(super) fn new() -> Self {
        Self {
            errors: Vec::new(),
            performances: Vec::new(),
            messages: Vec::new(),
            metrics: Vec::new(),
            cas_records: Vec::new(),
            daemon_logs: Vec::new(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.errors.is_empty()
            && self.performances.is_empty()
            && self.messages.is_empty()
            && self.metrics.is_empty()
            && self.cas_records.is_empty()
            && self.daemon_logs.is_empty()
    }

    pub(super) fn ingest_envelopes(&mut self, envelopes: Vec<TelemetryEnvelope>) {
        for envelope in envelopes {
            match envelope {
                TelemetryEnvelope::Error {
                    timestamp,
                    message,
                    context,
                } => {
                    self.errors.push(ErrorEvent {
                        timestamp,
                        message,
                        context,
                    });
                }
                TelemetryEnvelope::Performance {
                    timestamp,
                    operation,
                    duration_ms,
                    context,
                    tags,
                } => {
                    self.performances.push(PerformanceEvent {
                        timestamp,
                        operation,
                        duration_ms,
                        context,
                        tags,
                    });
                }
                TelemetryEnvelope::Message {
                    timestamp,
                    message,
                    level,
                    context,
                } => {
                    self.messages.push(MessageEvent {
                        timestamp,
                        message,
                        level,
                        context,
                    });
                }
                TelemetryEnvelope::Metrics { events } => {
                    self.metrics.extend(events);
                }
            }
        }
    }

    pub(super) fn ingest_cas(&mut self, records: Vec<CasSyncPayload>) {
        self.cas_records.extend(records);
    }

    pub(super) fn ingest_daemon_logs(&mut self, events: Vec<DaemonLogEvent>) {
        self.daemon_logs.extend(events);
        self.cap_daemon_logs();
    }

    pub(super) fn requeue_failed_daemon_logs(&mut self, mut failed_events: Vec<DaemonLogEvent>) {
        failed_events.append(&mut self.daemon_logs);
        self.daemon_logs = failed_events;
        self.cap_daemon_logs();
    }

    fn cap_daemon_logs(&mut self) {
        let overflow = self
            .daemon_logs
            .len()
            .saturating_sub(MAX_DAEMON_LOG_BUFFER_EVENTS);
        if overflow > 0 {
            self.daemon_logs.drain(0..overflow);
        }
    }

    pub(super) fn take(&mut self) -> TelemetryBuffer {
        TelemetryBuffer {
            errors: std::mem::take(&mut self.errors),
            performances: std::mem::take(&mut self.performances),
            messages: std::mem::take(&mut self.messages),
            metrics: std::mem::take(&mut self.metrics),
            cas_records: std::mem::take(&mut self.cas_records),
            daemon_logs: std::mem::take(&mut self.daemon_logs),
        }
    }
}
