//! Typed error for API HTTP responses.
//!
//! `ApiError` carries the operation name, HTTP status (if available), and a
//! human-readable message.  Its `retryability()` method classifies errors so
//! callers (e.g. `flush_pending_metric_records_with`) can route terminal auth
//! failures to `mark_undeliverable` rather than burning retry budget.
//!
//! The `From<ApiError> for GitAiError` bridge means every existing `pub fn …
//! -> Result<_, GitAiError>` boundary compiles unchanged — callers that do not
//! yet inspect retryability just see a regular `GitAiError`.

use crate::error::{GitAiError, Retryability};
use crate::model::api_types::ApiErrorResponse;
use std::fmt;

/// A structured HTTP-level error produced by the `clients/api` layer.
///
/// Fields are kept minimal: `operation` identifies the call site, `status`
/// carries the raw HTTP status code (absent for transport-level failures such
/// as DNS or connection reset), and `message` is the human-readable reason.
#[derive(Debug, Clone)]
pub struct ApiError {
    /// Short label for the operation, e.g. `"metrics upload"`.
    pub operation: &'static str,
    /// HTTP status code, if the server replied.
    pub status: Option<u16>,
    /// Human-readable error message.
    pub message: String,
}

impl ApiError {
    /// Classify this error for retry decisions.
    ///
    /// * 408, 429, 5xx and transport failures (no status) → `Retryable`
    /// * 4xx (excluding 408/429) → `Terminal`
    pub fn retryability(&self) -> Retryability {
        match self.status {
            // No status = transport failure (DNS, connect, timeout) — retryable
            None => Retryability::Retryable { retry_after: None },
            // Rate-limited or request-timeout — retryable
            Some(408) | Some(429) => Retryability::Retryable { retry_after: None },
            // Server errors — retryable
            Some(s) if s >= 500 => Retryability::Retryable { retry_after: None },
            // Auth/validation/not-found — permanent
            Some(_) => Retryability::Terminal,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.status {
            Some(status) => write!(
                f,
                "{} failed with status {}: {}",
                self.operation, status, self.message
            ),
            None => write!(f, "{} failed: {}", self.operation, self.message),
        }
    }
}

impl From<ApiError> for GitAiError {
    fn from(e: ApiError) -> Self {
        GitAiError::Api(e)
    }
}

/// Build an `ApiError` from an HTTP status code and response body.
///
/// Used by all endpoint modules to collapse the five near-identical per-status
/// match blocks into one shared call.  The `ApiErrorResponse` parse is
/// attempted; if it fails, `body` is used verbatim as the message.
pub(crate) fn http_status_error(
    operation: &'static str,
    status: u16,
    body: &str,
    default_message: &str,
) -> ApiError {
    let message = serde_json::from_str::<ApiErrorResponse>(body)
        .map(|r| r.error)
        .unwrap_or_else(|_| {
            if body.is_empty() {
                default_message.to_string()
            } else {
                body.to_string()
            }
        });
    ApiError {
        operation,
        status: Some(status),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_failure_is_retryable() {
        let e = ApiError {
            operation: "test",
            status: None,
            message: "connection refused".to_string(),
        };
        assert!(matches!(e.retryability(), Retryability::Retryable { .. }));
    }

    #[test]
    fn rate_limited_is_retryable() {
        for status in [408u16, 429] {
            let e = ApiError {
                operation: "test",
                status: Some(status),
                message: "too many requests".to_string(),
            };
            assert!(
                matches!(e.retryability(), Retryability::Retryable { .. }),
                "status {status} should be Retryable"
            );
        }
    }

    #[test]
    fn server_error_is_retryable() {
        for status in [500u16, 502, 503, 504] {
            let e = ApiError {
                operation: "test",
                status: Some(status),
                message: "server error".to_string(),
            };
            assert!(
                matches!(e.retryability(), Retryability::Retryable { .. }),
                "status {status} should be Retryable"
            );
        }
    }

    #[test]
    fn auth_error_is_terminal() {
        for status in [400u16, 401, 403, 404, 422] {
            let e = ApiError {
                operation: "test",
                status: Some(status),
                message: "unauthorized".to_string(),
            };
            assert!(
                matches!(e.retryability(), Retryability::Terminal),
                "status {status} should be Terminal"
            );
        }
    }

    #[test]
    fn display_with_status() {
        let e = ApiError {
            operation: "metrics upload",
            status: Some(401),
            message: "Unauthorized".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "metrics upload failed with status 401: Unauthorized"
        );
    }

    #[test]
    fn display_transport_error() {
        let e = ApiError {
            operation: "metrics upload",
            status: None,
            message: "connection refused".to_string(),
        };
        assert_eq!(e.to_string(), "metrics upload failed: connection refused");
    }

    #[test]
    fn http_status_error_parses_api_error_response() {
        let body = r#"{"error":"Bad Request","details":null}"#;
        let e = http_status_error("test op", 400, body, "fallback");
        assert_eq!(e.message, "Bad Request");
        assert_eq!(e.status, Some(400));
        assert_eq!(e.operation, "test op");
    }

    #[test]
    fn http_status_error_falls_back_to_body_on_bad_json() {
        let body = "not json";
        let e = http_status_error("test op", 500, body, "default");
        assert_eq!(e.message, "not json");
    }

    #[test]
    fn http_status_error_falls_back_to_default_on_empty_body() {
        let e = http_status_error("test op", 500, "", "Internal server error");
        assert_eq!(e.message, "Internal server error");
    }

    #[test]
    fn from_bridge_produces_api_variant() {
        let e = ApiError {
            operation: "metrics upload",
            status: Some(401),
            message: "Unauthorized".to_string(),
        };
        let wrapped: GitAiError = e.into();
        assert!(matches!(wrapped, GitAiError::Api(_)));
    }

    #[test]
    fn from_bridge_display_matches_inner() {
        let e = ApiError {
            operation: "metrics upload",
            status: Some(401),
            message: "Unauthorized".to_string(),
        };
        let display = e.to_string();
        let wrapped: GitAiError = ApiError {
            operation: "metrics upload",
            status: Some(401),
            message: "Unauthorized".to_string(),
        }
        .into();
        assert_eq!(wrapped.to_string(), display);
    }
}
