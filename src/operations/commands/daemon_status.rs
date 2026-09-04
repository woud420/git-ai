use super::*;

pub(super) fn handle_status(repo_working_dir: String) -> Result<(), String> {
    let config = daemon_config_from_env_or_default_paths()?;

    // Repository discovery remains separate because daemon-wide health must
    // also be available from a directory with no family identity.
    if crate::operations::git::find_repository_in_path(&repo_working_dir).is_err() {
        let daemon_running = daemon_is_up(&config);
        let mut output = serde_json::json!({
            "ok": true,
            "git_repo": false,
            "daemon_running": daemon_running,
        });
        if daemon_running {
            attach_daemon_health(&config, &mut output);
        }
        print_status(&output)?;
        return Ok(());
    }

    let response = send_control_request(
        &config.control_socket_path,
        &ControlRequest::StatusFamily { repo_working_dir },
    )
    .map_err(|error| error.to_string())?;
    let mut output = serde_json::to_value(response).map_err(|error| error.to_string())?;
    attach_daemon_health(&config, &mut output);
    print_status(&output)
}

fn attach_daemon_health(config: &DaemonConfig, output: &mut serde_json::Value) {
    match send_control_request(&config.control_socket_path, &ControlRequest::StatusDaemon) {
        Ok(response) if response.ok => {
            output["daemon"] = response.data.unwrap_or(serde_json::Value::Null);
        }
        Ok(response) => {
            output["daemon_error"] = serde_json::Value::String(daemon_health_error_message(
                response.error.unwrap_or_default(),
            ));
        }
        Err(error) => {
            output["daemon_error"] = serde_json::Value::String(error.to_string());
        }
    }
}

fn daemon_health_error_message(error: String) -> String {
    if error.contains("invalid control request") && error.contains("status.daemon") {
        "the running background service predates daemon health status; run `git-ai bg restart`"
            .to_string()
    } else {
        error
    }
}

fn print_status(output: &serde_json::Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(output).map_err(|error| error.to_string())?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_health_older_daemon_rejection_has_a_restart_hint() {
        let message = daemon_health_error_message(
            "invalid control request: unknown variant `status.daemon`".to_string(),
        );

        assert_eq!(
            message,
            "the running background service predates daemon health status; run `git-ai bg restart`"
        );
    }

    #[test]
    fn daemon_health_preserves_other_daemon_errors() {
        assert_eq!(
            daemon_health_error_message("health snapshot failed".to_string()),
            "health snapshot failed"
        );
    }
}
