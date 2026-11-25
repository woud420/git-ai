//! PostHog dispatch for buffered message events.

use super::buffer::{MessageEvent, extend_with_context};
use super::non_empty_setting;
use crate::config::Config;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const DEFAULT_POSTHOG_HOST: &str = "https://us.i.posthog.com";

/// The PostHog project one flush reports messages to.
pub(super) struct PostHogSink {
    api_key: String,
    host: String,
    distinct_id: String,
    agent: ureq::Agent,
}

impl PostHogSink {
    /// `None` unless telemetry is on and an API key is configured.
    pub(super) fn from_config(config: &Config, distinct_id: &str) -> Option<Self> {
        if !config.telemetry_enabled() {
            return None;
        }
        let api_key = non_empty_setting(
            std::env::var("POSTHOG_API_KEY").ok(),
            option_env!("POSTHOG_API_KEY"),
        )?;
        let host = non_empty_setting(
            std::env::var("POSTHOG_HOST").ok(),
            option_env!("POSTHOG_HOST"),
        )
        .unwrap_or_else(|| DEFAULT_POSTHOG_HOST.to_string());
        Some(Self {
            api_key,
            host,
            distinct_id: distinct_id.to_string(),
            agent: crate::clients::http::build_agent(Some(30)),
        })
    }

    /// Sends each message; after the first failed send, the rest are skipped.
    pub(super) fn send(&self, messages: &[MessageEvent]) {
        let endpoint = format!("{}/capture/", self.host.trim_end_matches('/'));
        for msg in messages {
            let event = posthog_capture_event(msg, &self.api_key, &self.distinct_id);
            let request = self
                .agent
                .post(&endpoint)
                .header("Content-Type", "application/json");
            let body = serde_json::to_string(&event).unwrap_or_default();
            if crate::clients::http::send_with_body(request, &body).is_err() {
                return;
            }
        }
    }
}

fn posthog_capture_event(msg: &MessageEvent, api_key: &str, distinct_id: &str) -> Value {
    let mut properties = BTreeMap::from([
        ("os".to_string(), json!(std::env::consts::OS)),
        ("arch".to_string(), json!(std::env::consts::ARCH)),
        ("version".to_string(), json!(env!("CARGO_PKG_VERSION"))),
        ("message".to_string(), json!(msg.message)),
        ("level".to_string(), json!(msg.level)),
    ]);
    extend_with_context(&mut properties, &msg.context);
    json!({
        "api_key": api_key,
        "event": msg.message,
        "properties": properties,
        "distinct_id": distinct_id,
        "timestamp": msg.timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OS: &str = std::env::consts::OS;
    const ARCH: &str = std::env::consts::ARCH;
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    fn message() -> MessageEvent {
        MessageEvent {
            timestamp: "2026-11-24T00:00:00Z".to_string(),
            message: "daemon started".to_string(),
            level: "warning".to_string(),
            context: Some(json!({"level": "from-context", "repo": "r"})),
        }
    }

    #[test]
    fn posthog_capture_event_lets_context_override_properties() {
        assert_eq!(
            posthog_capture_event(&message(), "phc_key", "id-1"),
            json!({
                "api_key": "phc_key",
                "event": "daemon started",
                "properties": {
                    "os": OS,
                    "arch": ARCH,
                    "version": VERSION,
                    "message": "daemon started",
                    "level": "from-context",
                    "repo": "r",
                },
                "distinct_id": "id-1",
                "timestamp": "2026-11-24T00:00:00Z",
            })
        );
    }
}
