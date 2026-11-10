use crate::metrics::MetricEvent;

#[doc(hidden)]
pub mod wltrace;

/// Maximum events per metrics envelope
pub const MAX_METRICS_PER_ENVELOPE: usize = 1000;

/// Submit telemetry envelopes via the best available path:
/// 1. External daemon control socket (wrapper processes)
/// 2. In-process daemon telemetry worker (daemon process itself)
/// 3. Silently drop if neither is available
fn submit_telemetry_envelope(envelopes: Vec<crate::model::telemetry::TelemetryEnvelope>) {
    if crate::operations::daemon::telemetry_handle::daemon_telemetry_available() {
        crate::operations::daemon::telemetry_handle::submit_telemetry(envelopes);
    } else if crate::operations::daemon::daemon_process_active() {
        crate::operations::daemon::telemetry_worker::submit_daemon_internal_telemetry(envelopes);
    }
}

/// Log an error to Sentry (via daemon telemetry worker)
pub fn log_error(error: &dyn std::error::Error, context: Option<serde_json::Value>) {
    let envelope = crate::model::telemetry::TelemetryEnvelope::Error {
        timestamp: chrono::Utc::now().to_rfc3339(),
        message: error.to_string(),
        context,
    };
    submit_telemetry_envelope(vec![envelope]);
}

/// Log a message to Sentry (info, warning, etc.) (via daemon telemetry worker)
#[allow(dead_code)]
pub fn log_message(message: &str, level: &str, context: Option<serde_json::Value>) {
    let envelope = crate::model::telemetry::TelemetryEnvelope::Message {
        timestamp: chrono::Utc::now().to_rfc3339(),
        message: message.to_string(),
        level: level.to_string(),
        context,
    };
    submit_telemetry_envelope(vec![envelope]);
}

/// Log a batch of metric events (via daemon telemetry worker).
///
/// Events are batched into envelopes of up to 1000 events each.
pub fn log_metrics(events: Vec<MetricEvent>) {
    #[cfg(any(test, feature = "test-support"))]
    {
        if std::env::var_os("GIT_AI_TEST_METRICS_DB_PATH").is_none() {
            return;
        }
    }

    if events.is_empty() {
        return;
    }

    // Split into chunks of MAX_METRICS_PER_ENVELOPE
    for chunk in events.chunks(MAX_METRICS_PER_ENVELOPE) {
        let envelope = crate::model::telemetry::TelemetryEnvelope::Metrics {
            events: chunk.to_vec(),
        };
        submit_telemetry_envelope(vec![envelope]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test error logging
    #[test]
    fn test_log_error_no_panic() {
        use std::io;
        let error = io::Error::new(io::ErrorKind::NotFound, "test error");
        log_error(&error, None);
    }

    #[test]
    fn test_log_error_with_context() {
        use serde_json::json;
        use std::io;
        let error = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let context = json!({"file": "test.txt", "operation": "read"});
        log_error(&error, Some(context));
    }

    // Test message logging
    #[test]
    fn test_log_message_basic() {
        log_message("test message", "info", None);
    }

    #[test]
    fn test_log_message_with_context() {
        use serde_json::json;
        let context = json!({"user": "test", "action": "login"});
        log_message("user logged in", "info", Some(context));
    }

    #[test]
    fn test_log_message_warning() {
        log_message("warning message", "warning", None);
    }

    // Test metrics logging
    #[test]
    fn test_log_metrics_empty() {
        log_metrics(vec![]);
    }

    // Test constants
    #[test]
    fn test_max_metrics_per_envelope() {
        assert_eq!(MAX_METRICS_PER_ENVELOPE, 1000);
    }
}
