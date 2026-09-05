//! In-memory buffer for telemetry events awaiting the next flush cycle.

use crate::metrics::MetricEvent;
use crate::model::api_types::DaemonLogEvent;
use crate::model::daemon_control::CasSyncPayload;
use crate::model::telemetry::TelemetryEnvelope;
use serde_json::Value;

pub(super) const MAX_BUFFERED_EVENTS_PER_KIND: usize = 5000;

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
        Self::cap_oldest(&mut self.errors);
        Self::cap_oldest(&mut self.performances);
        Self::cap_oldest(&mut self.messages);
        Self::cap_oldest(&mut self.metrics);
    }

    pub(super) fn ingest_cas(&mut self, records: Vec<CasSyncPayload>) {
        self.cas_records.extend(records);
        Self::cap_oldest(&mut self.cas_records);
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
        Self::cap_oldest(&mut self.daemon_logs);
    }

    fn cap_oldest<T>(events: &mut Vec<T>) {
        let overflow = events.len().saturating_sub(MAX_BUFFERED_EVENTS_PER_KIND);
        if overflow > 0 {
            events.drain(0..overflow);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_effort_buffers_drop_oldest_events_at_the_cap() {
        let mut buffer = TelemetryBuffer::new();
        let event_count = MAX_BUFFERED_EVENTS_PER_KIND + 1;

        buffer.ingest_envelopes(
            (0..event_count)
                .flat_map(|index| {
                    [
                        TelemetryEnvelope::Error {
                            timestamp: index.to_string(),
                            message: index.to_string(),
                            context: None,
                        },
                        TelemetryEnvelope::Performance {
                            timestamp: index.to_string(),
                            operation: index.to_string(),
                            duration_ms: index as u128,
                            context: None,
                            tags: None,
                        },
                        TelemetryEnvelope::Message {
                            timestamp: index.to_string(),
                            message: index.to_string(),
                            level: "info".to_string(),
                            context: None,
                        },
                    ]
                })
                .collect(),
        );
        buffer.ingest_cas(
            (0..event_count)
                .map(|index| CasSyncPayload {
                    hash: index.to_string(),
                    data: index.to_string(),
                    metadata: None,
                })
                .collect(),
        );

        assert_eq!(buffer.errors.len(), MAX_BUFFERED_EVENTS_PER_KIND);
        assert_eq!(buffer.performances.len(), MAX_BUFFERED_EVENTS_PER_KIND);
        assert_eq!(buffer.messages.len(), MAX_BUFFERED_EVENTS_PER_KIND);
        assert_eq!(buffer.cas_records.len(), MAX_BUFFERED_EVENTS_PER_KIND);
        assert_eq!(buffer.errors[0].message, "1");
        assert_eq!(buffer.performances[0].operation, "1");
        assert_eq!(buffer.messages[0].message, "1");
        assert_eq!(buffer.cas_records[0].hash, "1");
    }
}
