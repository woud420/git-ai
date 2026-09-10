use super::{ActorDaemonCoordinator, ControlRequest, ControlResponse};
use crate::model::jj_observer::{JjObserverControlReply, JjObserverError};

pub(super) async fn handle(
    coordinator: &ActorDaemonCoordinator,
    request: ControlRequest,
) -> ControlResponse {
    let action = match &request {
        ControlRequest::JjObserverEnable { .. } => "observer_enable",
        ControlRequest::JjObserverStatus => "observer_status",
        ControlRequest::JjObserverDisable => "observer_disable",
        ControlRequest::JjObserverResume => "observer_resume",
        _ => return ControlResponse::err("invalid observer control request"),
    };
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let reply = if let Some(observer) = &coordinator.jj_observer {
        match request {
            ControlRequest::JjObserverEnable {
                journal_path_hex,
                workspace_path_hex,
            } => observer.enable(journal_path_hex, workspace_path_hex).await,
            ControlRequest::JjObserverStatus => observer.status("status", "status", None),
            ControlRequest::JjObserverDisable => observer.disable().await,
            ControlRequest::JjObserverResume => observer.resume().await,
            _ => return ControlResponse::err("invalid observer control request"),
        }
    } else {
        unavailable(
            action,
            "intent_unavailable",
            "Observer worker is unavailable.",
        )
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let reply = {
        let _ = (coordinator, request);
        unavailable(
            action,
            "unsupported_platform",
            "Native jj observation requires Linux or macOS.",
        )
    };
    let error = reply.error.as_ref().map(|error| error.message.clone());
    match serde_json::to_value(reply) {
        Ok(value) => ControlResponse {
            ok: error.is_none(),
            seq: None,
            data: Some(value),
            error,
        },
        Err(_) => ControlResponse::err("observer reply serialization failed"),
    }
}

fn unavailable(action: &str, code: &str, message: &str) -> JjObserverControlReply {
    JjObserverControlReply {
        schema_version: 1,
        backend: "jj".into(),
        attribution_enabled: false,
        action: action.into(),
        disposition: "error".into(),
        revision: None,
        desired_intent: None,
        target: None,
        runtime: "disabled".into(),
        in_flight: false,
        session_cursor: None,
        last_error: None,
        error: Some(JjObserverError::new(code, message)),
    }
}
