use super::{Agent, formats, invalid_config};
use crate::error::GitAiError;
use crate::operations::mdm::file_ops::{generate_diff, write_atomic};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Ownership {
    config: PathBuf,
    socket: String,
}

pub(super) fn state_path(config: &Path) -> PathBuf {
    config.with_file_name(".git-ai-sandbox-socket.json")
}

fn read_optional(path: &Path) -> Result<Option<String>, GitAiError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn update(
    agent: Agent,
    desired: Option<&str>,
    dry_run: bool,
) -> Result<Option<String>, GitAiError> {
    let path = agent.config_path();
    let state_path = state_path(&path);
    let state_text = read_optional(&state_path)?;
    let previous: Option<Ownership> = state_text
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let before = read_optional(&path)?;
    let target = if before.is_some() {
        fs::canonicalize(&path)?
    } else {
        path.clone()
    };
    // A replaced symlink target is a different user configuration, even at the same path.
    let owned = previous
        .as_ref()
        .filter(|state| state.config == target)
        .map(|state| state.socket.as_str());
    let (after, socket) = if before.is_none() && desired.is_none() {
        (String::new(), None)
    } else {
        formats::update(agent, before.as_deref().unwrap_or(""), owned, desired)?
    };
    let next = socket.map(|socket| Ownership {
        config: target,
        socket,
    });
    let config_changed = before.as_deref().unwrap_or("") != after;
    if !config_changed && previous == next {
        return Ok(None);
    }
    let diff = generate_diff(&path, before.as_deref().unwrap_or(""), &after);
    if dry_run {
        return Ok(Some(diff));
    }
    if config_changed {
        write_atomic(&path, after.as_bytes())?;
    }
    let save_state = match next {
        Some(state) => write_atomic(&state_path, &serde_json::to_vec_pretty(&state)?),
        None if state_text.is_some() => fs::remove_file(&state_path).map_err(GitAiError::from),
        None => Ok(()),
    };
    if let Err(error) = save_state {
        // Never knowingly leave a new grant without its uninstall provenance.
        if config_changed {
            let rollback = match before {
                Some(content) => write_atomic(&path, content.as_bytes()),
                None => fs::remove_file(&path).map_err(GitAiError::from),
            };
            if let Err(rollback) = rollback {
                return Err(invalid_config(
                    &path,
                    format!("{error}; restoring previous configuration also failed: {rollback}"),
                ));
            }
        }
        return Err(error);
    }
    Ok(Some(diff))
}
