//! Sentry and PostHog event dispatch for telemetry error/performance/message events.

use super::buffer::{ErrorEvent, MessageEvent, PerformanceEvent};
use crate::config::Config;
use serde_json::{Value, json};
use std::cell::Cell;
use std::collections::BTreeMap;

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
            std::env::var("SENTRY_ENTERPRISE")
                .ok()
                .or_else(|| option_env!("SENTRY_ENTERPRISE").map(|s| s.to_string()))
                .filter(|s| !s.is_empty())
        });

    // Check for OSS DSN
    let oss_dsn = if !config.telemetry_enabled() {
        None
    } else {
        std::env::var("SENTRY_OSS")
            .ok()
            .or_else(|| option_env!("SENTRY_OSS").map(|s| s.to_string()))
            .filter(|s| !s.is_empty())
    };

    // Check for PostHog configuration
    let posthog_api_key = if !config.telemetry_enabled() {
        None
    } else {
        std::env::var("POSTHOG_API_KEY")
            .ok()
            .or_else(|| option_env!("POSTHOG_API_KEY").map(|s| s.to_string()))
            .filter(|s| !s.is_empty())
    };
    let posthog = posthog_api_key.map(|key| (key, crate::clients::http::build_agent(Some(30))));
    // After the first failed send, the rest of this flush skips PostHog.
    let mut posthog_failed = false;

    let posthog_host = std::env::var("POSTHOG_HOST")
        .ok()
        .or_else(|| option_env!("POSTHOG_HOST").map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://us.i.posthog.com".to_string());

    // Build Sentry clients
    let oss_client = oss_dsn.and_then(|dsn| SentryClient::from_dsn(&dsn));
    let enterprise_client = enterprise_dsn.and_then(|dsn| SentryClient::from_dsn(&dsn));

    // Build base tags
    let mut base_tags = BTreeMap::new();
    base_tags.insert("os".to_string(), json!(std::env::consts::OS));
    base_tags.insert("arch".to_string(), json!(std::env::consts::ARCH));
    base_tags.insert("distinct_id".to_string(), json!(distinct_id));

    // Send errors
    for error in errors {
        let mut extra = BTreeMap::new();
        if let Some(ctx) = &error.context
            && let Some(obj) = ctx.as_object()
        {
            for (key, value) in obj {
                extra.insert(key.clone(), value.clone());
            }
        }

        let event = json!({
            "message": error.message,
            "level": "error",
            "timestamp": error.timestamp,
            "platform": "other",
            "tags": base_tags,
            "extra": extra,
            "release": format!("git-ai@{}", env!("CARGO_PKG_VERSION")),
        });

        if let Some(client) = &oss_client {
            client.send_event(event.clone());
        }
        if let Some(client) = &enterprise_client {
            client.send_event(event);
        }
    }

    // Send performance events
    for perf in performances {
        let mut extra = BTreeMap::new();
        extra.insert("operation".to_string(), json!(perf.operation));
        extra.insert("duration_ms".to_string(), json!(perf.duration_ms));
        if let Some(ctx) = &perf.context
            && let Some(obj) = ctx.as_object()
        {
            for (key, value) in obj {
                extra.insert(key.clone(), value.clone());
            }
        }

        let mut perf_tags = base_tags.clone();
        if let Some(tags) = &perf.tags {
            for (key, value) in tags {
                perf_tags.insert(key.clone(), json!(value));
            }
        }

        let event = json!({
            "message": format!("Performance: {} ({}ms)", perf.operation, perf.duration_ms),
            "level": "info",
            "timestamp": perf.timestamp,
            "platform": "other",
            "tags": perf_tags,
            "extra": extra,
            "release": format!("git-ai@{}", env!("CARGO_PKG_VERSION")),
        });

        if let Some(client) = &oss_client {
            client.send_event(event.clone());
        }
        if let Some(client) = &enterprise_client {
            client.send_event(event);
        }
    }

    // Send messages (to Sentry + PostHog)
    for msg in messages {
        let mut extra = BTreeMap::new();
        if let Some(ctx) = &msg.context
            && let Some(obj) = ctx.as_object()
        {
            for (key, value) in obj {
                extra.insert(key.clone(), value.clone());
            }
        }

        let sentry_event = json!({
            "message": msg.message,
            "level": msg.level,
            "timestamp": msg.timestamp,
            "platform": "other",
            "tags": base_tags,
            "extra": extra,
            "release": format!("git-ai@{}", env!("CARGO_PKG_VERSION")),
        });

        if let Some(client) = &oss_client {
            client.send_event(sentry_event.clone());
        }
        if let Some(client) = &enterprise_client {
            client.send_event(sentry_event);
        }

        // PostHog only gets messages
        if let Some((api_key, agent)) = &posthog
            && !posthog_failed
        {
            let mut properties = BTreeMap::new();
            properties.insert("os".to_string(), json!(std::env::consts::OS));
            properties.insert("arch".to_string(), json!(std::env::consts::ARCH));
            properties.insert("version".to_string(), json!(env!("CARGO_PKG_VERSION")));
            properties.insert("message".to_string(), json!(msg.message));
            properties.insert("level".to_string(), json!(msg.level));
            if let Some(ctx) = &msg.context
                && let Some(obj) = ctx.as_object()
            {
                for (key, value) in obj {
                    properties.insert(key.clone(), value.clone());
                }
            }

            let endpoint = format!("{}/capture/", posthog_host.trim_end_matches('/'));
            let mut ph_event = json!({
                "api_key": api_key,
                "event": msg.message,
                "properties": properties,
                "distinct_id": distinct_id,
            });
            ph_event["timestamp"] = json!(msg.timestamp);

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
        let sent = crate::clients::http::send_with_body(request, &body)
            .is_ok_and(|response| (200..300).contains(&response.status_code));
        self.failed.set(!sent);
    }
}

#[cfg(test)]
mod tests {
    use super::SentryClient;

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

    #[test]
    fn send_event_skips_the_endpoint_after_its_first_failure() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let connections = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = connections.clone();
        // Accept and drop every connection: each request that reaches it fails.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                counter.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });

        let client = SentryClient::from_dsn(&format!("http://key@127.0.0.1:{port}/1")).unwrap();
        client.send_event(serde_json::json!({"message": "first"}));
        client.send_event(serde_json::json!({"message": "second"}));

        assert!(client.failed.get());
        assert_eq!(connections.load(Ordering::SeqCst), 1);
    }
}
