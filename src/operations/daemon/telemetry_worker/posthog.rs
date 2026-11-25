//! PostHog dispatch for buffered message events.

use super::buffer::{MessageEvent, extend_with_context};
use super::non_empty_setting;
use crate::config::Config;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const DEFAULT_POSTHOG_HOST: &str = "https://us.i.posthog.com";
/// Events per `/batch/` request, which keeps each body well under PostHog's
/// request size limit even with large message contexts.
const MAX_EVENTS_PER_BATCH: usize = 500;

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

    /// Sends the messages through PostHog's batch endpoint, in chunks; after
    /// the first failed request, the remaining chunks are skipped.
    pub(super) fn send(&self, messages: &[MessageEvent]) {
        let endpoint = format!("{}/batch/", self.host.trim_end_matches('/'));
        for chunk in messages.chunks(MAX_EVENTS_PER_BATCH) {
            let request = self
                .agent
                .post(&endpoint)
                .header("Content-Type", "application/json");
            let body = batch_body(&self.api_key, &self.distinct_id, chunk).to_string();
            if crate::clients::http::send_with_body(request, &body).is_err() {
                return;
            }
        }
    }
}

fn batch_body(api_key: &str, distinct_id: &str, messages: &[MessageEvent]) -> Value {
    let batch: Vec<Value> = messages
        .iter()
        .map(|msg| posthog_event(msg, distinct_id))
        .collect();
    json!({ "api_key": api_key, "batch": batch })
}

fn posthog_event(msg: &MessageEvent, distinct_id: &str) -> Value {
    let mut properties = BTreeMap::from([
        ("os".to_string(), json!(std::env::consts::OS)),
        ("arch".to_string(), json!(std::env::consts::ARCH)),
        ("version".to_string(), json!(env!("CARGO_PKG_VERSION"))),
        ("message".to_string(), json!(msg.message)),
        ("level".to_string(), json!(msg.level)),
    ]);
    extend_with_context(&mut properties, &msg.context);
    json!({
        "event": msg.message,
        "properties": properties,
        "distinct_id": distinct_id,
        "timestamp": msg.timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

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
    fn posthog_event_lets_context_override_properties() {
        assert_eq!(
            posthog_event(&message(), "id-1"),
            json!({
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

    /// A PostHog host that answers 200 and records each request body.
    fn recording_host() -> (String, Arc<Mutex<Vec<Value>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let recorded = bodies.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                let mut content_length = 0;
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap() > 2 {
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        content_length = value.trim().parse().unwrap();
                    }
                    line.clear();
                }
                let mut body = vec![0; content_length];
                reader.read_exact(&mut body).unwrap();
                recorded
                    .lock()
                    .unwrap()
                    .push(serde_json::from_slice(&body).unwrap());
                let _ = reader.get_mut().write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        (host, bodies)
    }

    #[test]
    fn send_posts_messages_in_batches_of_bounded_size() {
        let (host, bodies) = recording_host();
        let sink = PostHogSink {
            api_key: "phc_key".to_string(),
            host,
            distinct_id: "id-1".to_string(),
            agent: crate::clients::http::build_agent(Some(5)),
        };
        let messages: Vec<MessageEvent> =
            (0..MAX_EVENTS_PER_BATCH + 1).map(|_| message()).collect();

        sink.send(&messages);

        let bodies = bodies.lock().unwrap();
        let batch_sizes: Vec<usize> = bodies
            .iter()
            .map(|body| body["batch"].as_array().unwrap().len())
            .collect();
        assert_eq!(batch_sizes, vec![MAX_EVENTS_PER_BATCH, 1]);
        assert!(bodies.iter().all(|body| body["api_key"] == "phc_key"));
        assert_eq!(bodies[1]["batch"][0], posthog_event(&message(), "id-1"));
    }
}
