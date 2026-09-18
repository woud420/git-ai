use crate::operations::daemon::{
    DaemonConfig, control_api::ControlRequest, send_control_request_with_timeout,
};
use std::time::Duration;

pub(super) fn checkpoint_processing_pending() -> bool {
    let Ok(config) = DaemonConfig::from_env_or_default_paths() else {
        return false;
    };
    let Ok(response) = send_control_request_with_timeout(
        &config.control_socket_path,
        &ControlRequest::StatusDaemon,
        Duration::from_millis(500),
    ) else {
        return false;
    };
    response.ok
        && response
            .data
            .and_then(|data| {
                data.get("checkpoints_outstanding")
                    .and_then(serde_json::Value::as_u64)
            })
            .is_some_and(|count| count > 0)
}
