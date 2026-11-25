//! Sentry and PostHog event dispatch for telemetry error/performance/message events.

use super::buffer::{ErrorEvent, MessageEvent, PerformanceEvent};
use crate::config::Config;
use serde_json::{Value, json};
use std::cell::Cell;
use std::collections::BTreeMap;

/// A runtime environment value, else the value compiled into the binary;
/// an empty runtime value disables the setting.
fn non_empty_setting(runtime: Option<String>, compiled: Option<&str>) -> Option<String> {
    runtime
        .or_else(|| compiled.map(str::to_string))
        .filter(|s| !s.is_empty())
}

/// Flush buffered Sentry/PostHog events (errors, performance, messages).
pub(super) fn flush_sentry_and_posthog(
    config: &Config,
    distinct_id: &str,
    errors: &[ErrorEvent],
    performances: &[PerformanceEvent],
    messages: &[MessageEvent],
) {
    // Check for Enterprise DSN
    let enterprise_dsn = config
        .telemetry_enterprise_dsn()
        .map(|s| s.to_string())
        .or_else(|| {
            non_empty_setting(
                std::env::var("SENTRY_ENTERPRISE").ok(),
                option_env!("SENTRY_ENTERPRISE"),
            )
        });

    // Check for OSS DSN
    let oss_dsn = if !config.telemetry_enabled() {
        None
    } else {
        non_empty_setting(std::env::var("SENTRY_OSS").ok(), option_env!("SENTRY_OSS"))
    };

    // Check for PostHog configuration
    let posthog_api_key = if !config.telemetry_enabled() {
        None
    } else {
        non_empty_setting(
            std::env::var("POSTHOG_API_KEY").ok(),
            option_env!("POSTHOG_API_KEY"),
        )
    };
    let posthog = posthog_api_key.map(|key| (key, crate::clients::http::build_agent(Some(30))));
    // After the first failed send, the rest of this flush skips PostHog.
    let mut posthog_failed = false;

    let posthog_host = non_empty_setting(
        std::env::var("POSTHOG_HOST").ok(),
        option_env!("POSTHOG_HOST"),
    )
    .unwrap_or_else(|| "https://us.i.posthog.com".to_string());

    // Build Sentry clients
    let oss_client = oss_dsn.and_then(|dsn| SentryClient::from_dsn(&dsn));
    let enterprise_client = enterprise_dsn.and_then(|dsn| SentryClient::from_dsn(&dsn));

    let base_tags = base_tags(distinct_id);

    for error in errors {
        let event = sentry_error_event(error, &base_tags);
        if let Some(client) = &oss_client {
            client.send_event(event.clone());
        }
        if let Some(client) = &enterprise_client {
            client.send_event(event);
        }
    }

    for perf in performances {
        let event = sentry_performance_event(perf, &base_tags);
        if let Some(client) = &oss_client {
            client.send_event(event.clone());
        }
        if let Some(client) = &enterprise_client {
            client.send_event(event);
        }
    }

    // Messages go to Sentry and PostHog.
    for msg in messages {
        let sentry_event = sentry_message_event(msg, &base_tags);
        if let Some(client) = &oss_client {
            client.send_event(sentry_event.clone());
        }
        if let Some(client) = &enterprise_client {
            client.send_event(sentry_event);
        }

        if let Some((api_key, agent)) = &posthog
            && !posthog_failed
        {
            let endpoint = format!("{}/capture/", posthog_host.trim_end_matches('/'));
            let ph_event = posthog_capture_event(msg, api_key, distinct_id);
            let request = agent
                .post(&endpoint)
                .header("Content-Type", "application/json");
            posthog_failed = crate::clients::http::send_with_body(
                request,
                &serde_json::to_string(&ph_event).unwrap_or_default(),
            )
            .is_err();
        }
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

/// Copies the entries of an event's JSON object context into `target`,
/// overwriting keys already there.
fn extend_with_context(target: &mut BTreeMap<String, Value>, context: &Option<Value>) {
    if let Some(Value::Object(entries)) = context {
        for (key, value) in entries {
            target.insert(key.clone(), value.clone());
        }
    }
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

/// Minimal Sentry client for sending events to a Sentry DSN, built once per flush.
struct SentryClient {
    endpoint: String,
    public_key: String,
    agent: ureq::Agent,
    /// Set by the first failed send; the rest of this flush skips the endpoint.
    failed: Cell<bool>,
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
            failed: Cell::new(false),
        })
    }

    fn send_event(&self, event: Value) {
        if self.failed.get() {
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
        let body = serde_json::to_string(&event).unwrap_or_default();
        // Only an unreachable endpoint ends the flush: an event the server
        // rejects says nothing about the events after it.
        self.failed
            .set(crate::clients::http::send_with_body(request, &body).is_err());
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

#[cfg(test)]
mod tests {
    use super::SentryClient;
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
    fn send_event_skips_the_endpoint_after_a_transport_failure() {
        let (client, connections) = client_answered_with("");
        client.send_event(serde_json::json!({"message": "first"}));
        client.send_event(serde_json::json!({"message": "second"}));

        assert!(client.failed.get());
        assert_eq!(connections.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn send_event_keeps_sending_after_a_rejected_event() {
        let (client, connections) = client_answered_with(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        client.send_event(serde_json::json!({"message": "first"}));
        client.send_event(serde_json::json!({"message": "second"}));

        assert!(!client.failed.get());
        assert_eq!(connections.load(Ordering::SeqCst), 2);
    }
}
