mod formats;
mod ownership;

use super::hook_installer::{InstallResult, UninstallResult};
use super::paths::{claude_config_dir, codex_home_dir};
use crate::config::Config;
use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use crate::operations::daemon::DaemonConfig;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum Agent {
    Codex,
    Claude,
}

impl Agent {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "codex" => Some(Self::Codex),
            "claude-code" => Some(Self::Claude),
            _ => None,
        }
    }

    fn config_path(self) -> PathBuf {
        match self {
            Self::Codex => codex_home_dir().join("config.toml"),
            Self::Claude => claude_config_dir().join("settings.json"),
        }
    }

    fn supported(self) -> bool {
        match self {
            Self::Codex => cfg!(unix),
            Self::Claude => cfg!(target_os = "macos"),
        }
    }
}

pub(crate) fn has_ownership(id: &str) -> bool {
    Agent::from_id(id).is_some_and(|agent| ownership::state_path(&agent.config_path()).exists())
}

pub(super) fn install(id: &str, dry_run: bool) -> Result<Vec<InstallResult>, GitAiError> {
    let Some(agent) = Agent::from_id(id) else {
        return Ok(vec![]);
    };
    if !Config::fresh().feature_flags().whitelist_agent_sandboxes {
        return Ok(vec![]);
    }
    if !agent.supported() {
        return Ok(vec![InstallResult {
            changed: false,
            diff: None,
            message: format!(
                "Unable to configure a path-scoped Unix socket permission for {id} on this platform"
            ),
        }]);
    }
    let socket = DaemonConfig::from_env_or_default_paths()?.trace_socket_path;
    let socket = socket
        .to_str()
        .filter(|_| socket.is_absolute())
        .ok_or_else(|| {
            invalid_config(&socket, "the trace socket must be an absolute UTF-8 path")
        })?;
    let diff = ownership::update(agent, Some(socket), dry_run)?;
    Ok(vec![InstallResult {
        changed: diff.is_some(),
        message: if diff.is_some() {
            format!("{id}: active trace socket permission updated")
        } else {
            format!("{id}: active trace socket permission already configured")
        },
        diff,
    }])
}

pub(super) fn uninstall(id: &str, dry_run: bool) -> Result<Vec<UninstallResult>, GitAiError> {
    let Some(agent) = Agent::from_id(id) else {
        return Ok(vec![]);
    };
    if !has_ownership(id) {
        return Ok(vec![]);
    }
    let diff = ownership::update(agent, None, dry_run)?;
    Ok(vec![UninstallResult {
        changed: diff.is_some(),
        diff,
        message: format!("{id}: Git AI-owned socket permission cleanup complete"),
    }])
}

fn invalid_config(path: &Path, message: impl Into<String>) -> GitAiError {
    PersistenceError::Io {
        operation: "sandbox socket configuration",
        path: path.display().to_string(),
        kind: std::io::ErrorKind::InvalidData,
        message: message.into(),
    }
    .into()
}
