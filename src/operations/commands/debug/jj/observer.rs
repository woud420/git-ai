use super::{Error, observer_args::Request, shared};
use crate::cli::{print_machine_json, print_machine_json_serializable};
use crate::model::jj_observer::{JjObserverControlReply, paths};
use crate::operations::daemon::{
    ControlRequest, DaemonConfig, send_control_request, send_control_request_with_timeout,
};
use std::time::Duration;

mod reply;

pub(super) fn run(request: Request<'_>) -> i32 {
    match execute(request) {
        Ok(reply) => {
            let code = i32::from(reply.error.is_some());
            print_machine_json_serializable(&reply);
            code
        }
        Err(error) => {
            print_machine_json(&error.value());
            1
        }
    }
}

fn unavailable() -> Error {
    Error::new("daemon_unavailable", "Observer daemon is unavailable.")
}

fn execute(request: Request<'_>) -> Result<JjObserverControlReply, Error> {
    let (request, action, start) = match request {
        Request::Enable { journal } => {
            shared::ordinary_journal_path(journal)?;
            let cwd = std::env::current_dir().map_err(|_| {
                Error::new("context_unavailable", "Current directory is unavailable.")
            })?;
            let journal = if journal.is_absolute() {
                journal.to_owned()
            } else {
                cwd.join(journal)
            };
            let journal_path_hex = paths::encode(&journal)
                .map_err(|error| Error::new("journal_unavailable", error))?;
            let workspace_path_hex =
                paths::encode(&cwd).map_err(|error| Error::new("context_unavailable", error))?;
            (
                ControlRequest::JjObserverEnable {
                    journal_path_hex,
                    workspace_path_hex,
                },
                "observer_enable",
                true,
            )
        }
        Request::Status => (ControlRequest::JjObserverStatus, "observer_status", false),
        Request::Disable => (ControlRequest::JjObserverDisable, "observer_disable", false),
        Request::Resume => (ControlRequest::JjObserverResume, "observer_resume", true),
    };
    let config = if start {
        crate::operations::commands::daemon::ensure_daemon_running(Duration::from_secs(5))
            .map_err(|_| unavailable())?
    } else {
        DaemonConfig::from_env_or_default_paths().map_err(|_| unavailable())?
    };
    let response = if start {
        send_control_request_with_timeout(
            &config.control_socket_path,
            &request,
            Duration::from_secs(10),
        )
    } else {
        send_control_request(&config.control_socket_path, &request)
    }
    .map_err(|_| unavailable())?;
    let value = response.data.ok_or_else(unavailable)?;
    let reply = serde_json::from_value(value).map_err(|_| unavailable())?;
    reply::validate(&reply, action, response.ok).map_err(|_| unavailable())?;
    Ok(reply)
}
