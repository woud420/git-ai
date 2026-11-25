//! Sentry dispatch for buffered error, performance, and message events.

use super::buffer::{ErrorEvent, MessageEvent, PerformanceEvent, extend_with_context};
use super::non_empty_setting;
use crate::config::Config;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// The Sentry projects one flush reports to.
pub(super) struct SentrySink {
    clients: Vec<SentryClient>,
    base_tags: BTreeMap<String, Value>,
}

impl SentrySink {
    /// The OSS project (when telemetry is on) and any enterprise project;
    /// `None` when neither is configured.
    pub(super) fn from_config(config: &Config, distinct_id: &str) -> Option<Self> {
        let enterprise_dsn = config
            .telemetry_enterprise_dsn()
            .map(|s| s.to_string())
            .or_else(|| {
                non_empty_setting(
                    std::env::var("SENTRY_ENTERPRISE").ok(),
                    option_env!("SENTRY_ENTERPRISE"),
                )
            });
        let oss_dsn = if config.telemetry_enabled() {
            non_empty_setting(std::env::var("SENTRY_OSS").ok(), option_env!("SENTRY_OSS"))
        } else {
            None
        };
        let clients: Vec<SentryClient> = [oss_dsn, enterprise_dsn]
            .into_iter()
            .flatten()
            .filter_map(|dsn| SentryClient::from_dsn(&dsn))
            .collect();
        (!clients.is_empty()).then(|| Self {
            clients,
            base_tags: base_tags(distinct_id),
        })
    }

    pub(super) fn send(
        &self,
        errors: &[ErrorEvent],
        performances: &[PerformanceEvent],
        messages: &[MessageEvent],
    ) {
        let events = errors
            .iter()
            .map(|error| sentry_error_event(error, &self.base_tags))
            .chain(
                performances
                    .iter()
                    .map(|perf| sentry_performance_event(perf, &self.base_tags)),
            )
            .chain(
                messages
                    .iter()
                    .map(|msg| sentry_message_event(msg, &self.base_tags)),
            );
        let events: Vec<Value> = events.collect();
        // One thread per project, so an unreachable project does not delay
        // the others; each still sends its events in order.
        std::thread::scope(|scope| {
            for client in &self.clients {
                scope.spawn(|| {
                    for event in &events {
                        client.send_event(event);
                    }
                });
            }
        });
    }
}

fn base_tags(distinct_id: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("os".to_string(), json!(std::env::consts::OS)),
        ("arch".to_string(), json!(std::env::consts::ARCH)),
        ("distinct_id".to_string(), json!(distinct_id)),
    ])
}

fn sentry_release() -> String {
    format!("git-ai@{}", env!("CARGO_PKG_VERSION"))
}

fn sentry_error_event(error: &ErrorEvent, base_tags: &BTreeMap<String, Value>) -> Value {
    let mut extra = BTreeMap::new();
    extend_with_context(&mut extra, &error.context);
    json!({
        "message": error.message,
        "level": "error",
        "timestamp": error.timestamp,
        "platform": "other",
        "tags": base_tags,
        "extra": extra,
        "release": sentry_release(),
    })
}

fn sentry_performance_event(perf: &PerformanceEvent, base_tags: &BTreeMap<String, Value>) -> Value {
    let mut extra = BTreeMap::new();
    extra.insert("operation".to_string(), json!(perf.operation));
    extra.insert("duration_ms".to_string(), json!(perf.duration_ms));
    extend_with_context(&mut extra, &perf.context);

    let mut tags = base_tags.clone();
    for (key, value) in perf.tags.iter().flatten() {
        tags.insert(key.clone(), json!(value));
    }

    json!({
        "message": format!("Performance: {} ({}ms)", perf.operation, perf.duration_ms),
        "level": "info",
        "timestamp": perf.timestamp,
        "platform": "other",
        "tags": tags,
        "extra": extra,
        "release": sentry_release(),
    })
}

fn sentry_message_event(msg: &MessageEvent, base_tags: &BTreeMap<String, Value>) -> Value {
    let mut extra = BTreeMap::new();
    extend_with_context(&mut extra, &msg.context);
    json!({
        "message": msg.message,
        "level": msg.level,
        "timestamp": msg.timestamp,
        "platform": "other",
        "tags": base_tags,
        "extra": extra,
        "release": sentry_release(),
    })
}

/// Minimal Sentry client for sending events to a Sentry DSN, built once per flush.
struct SentryClient {
    endpoint: String,
    public_key: String,
    agent: ureq::Agent,
    /// Set by the first failed send; the rest of this flush skips the endpoint.
    failed: AtomicBool,
}

impl SentryClient {
    fn from_dsn(dsn: &str) -> Option<Self> {
        let url = url::Url::parse(dsn).ok()?;
        let public_key = url.username().to_string();
        let host = url.host_str()?;
        let port = url
            .port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default();
        // `{scheme}://{key}@{host}[:{port}]/{path-prefix/}{project_id}`
        let path = url.path().trim_matches('/');
        let (prefix, project_id) = path.split_at(path.rfind('/').map_or(0, |i| i + 1));
        let endpoint = format!(
            "{}://{host}{port}/{prefix}api/{project_id}/store/",
            url.scheme()
        );
        Some(SentryClient {
            endpoint,
            public_key,
            agent: crate::clients::http::build_agent(Some(30)),
            failed: AtomicBool::new(false),
        })
    }

    fn send_event(&self, event: &Value) {
        if self.failed.load(Ordering::Relaxed) {
            return;
        }
        let auth_header = format!(
            "Sentry sentry_version=7, sentry_key={}, sentry_client=git-ai/{}",
            self.public_key,
            env!("CARGO_PKG_VERSION")
        );

        let request = self
            .agent
            .post(&self.endpoint)
            .header("X-Sentry-Auth", &auth_header)
            .header("Content-Type", "application/json");
        let body = serde_json::to_string(event).unwrap_or_default();
        // Only an unreachable endpoint ends the flush: an event the server
        // rejects says nothing about the events after it.
        self.failed.store(
            crate::clients::http::send_with_body(request, &body).is_err(),
            Ordering::Relaxed,
        );
    }
}

#[cfg(test)]
mod payload_tests {
    use super::*;
    use std::collections::HashMap;

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
    fn sentry_error_event_copies_object_context_into_extra() {
        let error = ErrorEvent {
            timestamp: "t".to_string(),
            message: "boom".to_string(),
            context: Some(json!({"file": "a.rs"})),
        };
        assert_eq!(
            sentry_error_event(&error, &base_tags("id-1")),
            json!({
                "message": "boom",
                "level": "error",
                "timestamp": "t",
                "platform": "other",
                "tags": {"os": OS, "arch": ARCH, "distinct_id": "id-1"},
                "extra": {"file": "a.rs"},
                "release": format!("git-ai@{VERSION}"),
            })
        );
    }

    #[test]
    fn sentry_error_event_ignores_non_object_context() {
        let error = ErrorEvent {
            timestamp: "t".to_string(),
            message: "boom".to_string(),
            context: Some(json!(["not", "an", "object"])),
        };
        assert_eq!(
            sentry_error_event(&error, &base_tags("id-1"))["extra"],
            json!({})
        );
    }

    #[test]
    fn sentry_performance_event_lets_context_and_tags_override_defaults() {
        let perf = PerformanceEvent {
            timestamp: "t".to_string(),
            operation: "checkpoint".to_string(),
            duration_ms: 12,
            context: Some(json!({"duration_ms": "from-context", "files": 3})),
            tags: Some(HashMap::from([("os".to_string(), "custom".to_string())])),
        };
        assert_eq!(
            sentry_performance_event(&perf, &base_tags("id-1")),
            json!({
                "message": "Performance: checkpoint (12ms)",
                "level": "info",
                "timestamp": "t",
                "platform": "other",
                "tags": {"os": "custom", "arch": ARCH, "distinct_id": "id-1"},
                "extra": {"operation": "checkpoint", "duration_ms": "from-context", "files": 3},
                "release": format!("git-ai@{VERSION}"),
            })
        );
    }

    #[test]
    fn sentry_message_event_keeps_the_message_level() {
        assert_eq!(
            sentry_message_event(&message(), &base_tags("id-1")),
            json!({
                "message": "daemon started",
                "level": "warning",
                "timestamp": "2026-11-24T00:00:00Z",
                "platform": "other",
                "tags": {"os": OS, "arch": ARCH, "distinct_id": "id-1"},
                "extra": {"level": "from-context", "repo": "r"},
                "release": format!("git-ai@{VERSION}"),
            })
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorEvent, SentryClient, SentrySink};
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn from_dsn_keeps_port_and_path_prefix() {
        let endpoint = |dsn: &str| SentryClient::from_dsn(dsn).unwrap().endpoint;
        assert_eq!(
            endpoint("https://key@o1.ingest.sentry.io/42"),
            "https://o1.ingest.sentry.io/api/42/store/"
        );
        assert_eq!(
            endpoint("https://key@sentry.corp:9000/sentry/7"),
            "https://sentry.corp:9000/sentry/api/7/store/"
        );
    }

    /// A client whose endpoint answers each connection with `response` (or
    /// just closes it when empty), and the number of connections it received.
    fn client_answered_with(response: &'static str) -> (SentryClient, Arc<AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = connections.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                counter.fetch_add(1, Ordering::SeqCst);
                if !response.is_empty() {
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    let _ = std::io::copy(&mut stream, &mut std::io::sink());
                }
            }
        });
        let client = SentryClient::from_dsn(&format!("http://key@127.0.0.1:{port}/1")).unwrap();
        (client, connections)
    }

    #[test]
    fn sink_sends_every_event_to_every_project_even_when_one_is_down() {
        const REJECTED: &str =
            "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (up, up_connections) = client_answered_with(REJECTED);
        let (down, down_connections) = client_answered_with("");
        let sink = SentrySink {
            clients: vec![down, up],
            base_tags: BTreeMap::new(),
        };
        let error = |message: &str| ErrorEvent {
            timestamp: "t".to_string(),
            message: message.to_string(),
            context: None,
        };

        sink.send(&[error("first"), error("second")], &[], &[]);

        assert_eq!(up_connections.load(Ordering::SeqCst), 2);
        assert_eq!(down_connections.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn send_event_skips_the_endpoint_after_a_transport_failure() {
        let (client, connections) = client_answered_with("");
        client.send_event(&serde_json::json!({"message": "first"}));
        client.send_event(&serde_json::json!({"message": "second"}));

        assert!(client.failed.load(Ordering::SeqCst));
        assert_eq!(connections.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn send_event_keeps_sending_after_a_rejected_event() {
        let (client, connections) = client_answered_with(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        client.send_event(&serde_json::json!({"message": "first"}));
        client.send_event(&serde_json::json!({"message": "second"}));

        assert!(!client.failed.load(Ordering::SeqCst));
        assert_eq!(connections.load(Ordering::SeqCst), 2);
    }
}
