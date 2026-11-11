#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartBlockReason {
    AutoStartDisabled,
    StrongSandboxMarker,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxMarkers {
    pub cursor_sandbox: Option<String>,
    pub sandbox_runtime: Option<String>,
    pub codex_sandbox: Option<String>,
}

impl SandboxMarkers {
    pub const ENV_VARS: [&'static str; 4] = [
        "CURSOR_SANDBOX",
        "SANDBOX_RUNTIME",
        "CODEX_SANDBOX",
        "CODEX_SANDBOX_NETWORK_DISABLED",
    ];

    pub fn from_env() -> Self {
        Self {
            cursor_sandbox: std::env::var("CURSOR_SANDBOX").ok(),
            sandbox_runtime: std::env::var("SANDBOX_RUNTIME").ok(),
            codex_sandbox: std::env::var("CODEX_SANDBOX").ok(),
        }
    }

    pub fn strong_marker_name(&self) -> Option<&'static str> {
        if self.cursor_sandbox.is_some() {
            Some("CURSOR_SANDBOX")
        } else if self.sandbox_runtime.is_some() {
            Some("SANDBOX_RUNTIME")
        } else if self.codex_sandbox.is_some() {
            Some("CODEX_SANDBOX")
        } else {
            None
        }
    }

    pub fn has_strong_marker(&self) -> bool {
        self.strong_marker_name().is_some()
    }
}

/// Why a detached daemon start is not allowed, if it is not.
fn start_block_reason(
    auto_start_disabled: bool,
    markers: &SandboxMarkers,
) -> Option<StartBlockReason> {
    if auto_start_disabled {
        Some(StartBlockReason::AutoStartDisabled)
    } else if markers.has_strong_marker() {
        Some(StartBlockReason::StrongSandboxMarker)
    } else {
        None
    }
}

fn blocked_message(reason: StartBlockReason, markers: &SandboxMarkers) -> String {
    match reason {
        StartBlockReason::AutoStartDisabled => {
            "daemon auto-spawn disabled (_GITAI_INTERNAL_DISABLE_WRAPPER_DAEMON_AUTOSPAWN)"
                .to_string()
        }
        StartBlockReason::StrongSandboxMarker => {
            let marker = markers.strong_marker_name().unwrap_or("sandbox marker");
            format!(
                "daemon startup blocked inside a sandbox ({marker} is set); run the daemon outside the sandbox or use checkpoint delivery outbox"
            )
        }
    }
}

/// `Err` with a user-facing message when a detached daemon start is blocked.
pub fn require_detached_start_allowed(
    auto_start_disabled: bool,
    markers: &SandboxMarkers,
) -> Result<(), String> {
    match start_block_reason(auto_start_disabled, markers) {
        Some(reason) => Err(blocked_message(reason, markers)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_auto_start_disable_blocks_start() {
        assert_eq!(
            start_block_reason(true, &SandboxMarkers::default()),
            Some(StartBlockReason::AutoStartDisabled)
        );
    }

    #[test]
    fn each_strong_marker_blocks_start() {
        for markers in [
            SandboxMarkers {
                cursor_sandbox: Some("1".to_string()),
                ..Default::default()
            },
            SandboxMarkers {
                sandbox_runtime: Some("sandbox-exec".to_string()),
                ..Default::default()
            },
            SandboxMarkers {
                codex_sandbox: Some("seatbelt".to_string()),
                ..Default::default()
            },
        ] {
            assert_eq!(
                start_block_reason(false, &markers),
                Some(StartBlockReason::StrongSandboxMarker)
            );
        }
    }

    #[test]
    fn no_markers_allow_start() {
        assert_eq!(start_block_reason(false, &SandboxMarkers::default()), None);
        assert!(require_detached_start_allowed(false, &SandboxMarkers::default()).is_ok());
    }

    #[test]
    fn reports_the_first_strong_marker_without_exposing_its_value() {
        let markers = SandboxMarkers {
            cursor_sandbox: Some("secret-value".to_string()),
            sandbox_runtime: Some("seatbelt".to_string()),
            ..Default::default()
        };

        assert_eq!(markers.strong_marker_name(), Some("CURSOR_SANDBOX"));
        let message = require_detached_start_allowed(false, &markers).unwrap_err();
        assert!(message.contains("CURSOR_SANDBOX"), "{message}");
        assert!(!message.contains("secret-value"), "{message}");
    }
}
