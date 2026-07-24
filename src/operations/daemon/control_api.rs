//! Re-exports daemon control-socket DTOs from their canonical model location,
//! plus status-query behavior built on top of them.
//!
//! All wire-shape types live in `crate::model::daemon_control`; this module
//! keeps the `crate::operations::daemon::control_api::*` import paths valid
//! for the 16+ existing consumers and hosts the family-status polling helpers
//! used by the `git-ai debug` self-checks.
pub use crate::model::daemon_control::{
    BashSessionQueryResponse, BashSnapshotQueryResponse, CasSyncPayload, ControlRequest,
    ControlResponse, FamilyStatus,
};

use std::path::Path;
use std::time::{Duration, Instant};

const DAEMON_CONTROL_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) fn wait_for_daemon_family_status(
    config: &crate::operations::daemon::DaemonConfig,
    repo_path: &Path,
    expected_min_seq: u64,
    deadline: Instant,
) -> Result<FamilyStatus, String> {
    let mut last_error = None;

    while Instant::now() < deadline {
        match read_daemon_family_status(config, repo_path) {
            Ok(status) if status.latest_seq >= expected_min_seq => return Ok(status),
            Ok(status) => {
                last_error = Some(format!(
                    "latest_seq={}, expected at least {}, last_error={}",
                    status.latest_seq,
                    expected_min_seq,
                    status.last_error.as_deref().unwrap_or("<none>")
                ));
            }
            Err(err) => last_error = Some(err),
        }
        std::thread::sleep(crate::operations::daemon::self_check::POLL_INTERVAL);
    }

    Err(format!(
        "timed out waiting for daemon family status: {}",
        last_error.unwrap_or_else(|| format!("no status for {}", repo_path.display()))
    ))
}

fn read_daemon_family_status(
    config: &crate::operations::daemon::DaemonConfig,
    repo_path: &Path,
) -> Result<FamilyStatus, String> {
    let request = ControlRequest::StatusFamily {
        repo_working_dir: repo_path.display().to_string(),
    };
    let response = crate::operations::daemon::send_control_request_with_timeout(
        &config.control_socket_path,
        &request,
        DAEMON_CONTROL_TIMEOUT,
    )
    .map_err(|e| e.to_string())?;

    if !response.ok {
        return Err(response
            .error
            .unwrap_or_else(|| "daemon status request failed".to_string()));
    }

    let data = response
        .data
        .ok_or_else(|| "daemon status response had no data".to_string())?;
    serde_json::from_value::<FamilyStatus>(data).map_err(|e| e.to_string())
}

pub(crate) fn daemon_family_status_detail(repo_path: &Path) -> String {
    let config = match crate::operations::daemon::DaemonConfig::from_env_or_default_paths() {
        Ok(config) => config,
        Err(err) => {
            return format!("daemon status for repo: <error: {}>", err);
        }
    };

    match read_daemon_family_status(&config, repo_path) {
        Ok(status) => format!(
            "daemon status for repo: latest_seq={}, last_error={}",
            status.latest_seq,
            status.last_error.as_deref().unwrap_or("<none>")
        ),
        Err(err) => format!("daemon status for repo: <error: {}>", err),
    }
}
