use super::cli::InstallOptions;
use super::{
    TRACE2_EVENT_NESTING_KEY, TRACE2_EVENT_NESTING_VALUE, TRACE2_EVENT_TARGET_KEY,
    VISUAL_STUDIO_INSTALLER_ID,
};
use crate::config;
use crate::error::GitAiError;
use crate::operations::daemon::DaemonConfig;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(super) fn set_global_git_config_value(
    git_cmd: &str,
    key: &str,
    value: &str,
) -> Result<(), GitAiError> {
    let mut command = Command::new(git_cmd);
    command
        .args(["config", "--global", key, value])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::clients::git_cli::apply_internal_git_env(&mut command);

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(GitAiError::Generic(format!(
            "failed to set global git config key '{}'",
            key
        )))
    }
}

pub(super) fn ensure_global_git_config_dirs() -> Result<(), GitAiError> {
    if let Ok(path) = std::env::var("GIT_CONFIG_GLOBAL") {
        let config_path = PathBuf::from(path);
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        fs::create_dir_all(home)?;
    }

    Ok(())
}

pub(crate) fn remove_global_git_config_section(
    git_cmd: &str,
    section: &str,
) -> Result<(), GitAiError> {
    let mut command = Command::new(git_cmd);
    command
        .args(["config", "--global", "--remove-section", section])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::clients::git_cli::apply_internal_git_env(&mut command);

    let status = command.status()?;
    // Exit code 128 means the section doesn't exist, which is fine.
    if status.success() || status.code() == Some(128) {
        Ok(())
    } else {
        Err(GitAiError::Generic(format!(
            "failed to remove global git config section '{}'",
            section
        )))
    }
}

pub(super) fn configure_daemon_trace2(dry_run: bool) -> Result<(), GitAiError> {
    let runtime_config = config::Config::fresh();

    ensure_global_git_config_dirs()?;

    let daemon_config = DaemonConfig::from_env_or_default_paths()?;
    let event_target = daemon_config.trace2_event_target();

    if dry_run {
        return Ok(());
    }

    // Fully reset any existing trace2 config the user may have set
    // (e.g. trace2.normalTarget, trace2.perfTarget, trace2.configParams, etc.)
    // before writing only the keys we need.
    remove_global_git_config_section(runtime_config.git_cmd(), "trace2")?;

    set_global_git_config_value(
        runtime_config.git_cmd(),
        TRACE2_EVENT_TARGET_KEY,
        &event_target,
    )?;
    set_global_git_config_value(
        runtime_config.git_cmd(),
        TRACE2_EVENT_NESTING_KEY,
        TRACE2_EVENT_NESTING_VALUE,
    )?;
    Ok(())
}

pub(super) fn ensure_daemon(dry_run: bool) {
    if dry_run {
        return;
    }

    // Don't touch daemon inside test harnesses
    if std::env::var_os("GIT_AI_TEST_DB_PATH").is_some()
        || std::env::var_os("GITAI_TEST_DB_PATH").is_some()
    {
        return;
    }

    let Ok(daemon_config) = DaemonConfig::from_env_or_default_paths() else {
        return;
    };

    // Restart daemon so it picks up the freshly-written trace2 config.
    // Uses soft shutdown → hard kill escalation if needed.
    if let Err(e) = crate::operations::commands::daemon::restart_daemon(&daemon_config) {
        eprintln!(
            "[git-ai] warning: failed to restart background service: {}",
            e
        );
    }
}

pub(super) fn should_include_installer(id: &str, options: &InstallOptions) -> bool {
    options.include_visual_studio_extension || id != VISUAL_STUDIO_INSTALLER_ID
}

#[derive(Default)]
pub(super) struct InstallConfig {
    pub(super) api_base: Option<String>,
    pub(super) api_key: Option<String>,
}

pub(super) fn persist_install_config_with_values(
    binary_path: &Path,
    dry_run: bool,
    install_config: &InstallConfig,
) -> Result<bool, GitAiError> {
    if dry_run {
        return Ok(false);
    }

    let api_base = &install_config.api_base;
    let api_key = &install_config.api_key;

    if api_base.is_none() && api_key.is_none() {
        return Ok(false);
    }

    let mut file_config = crate::config::load_file_config_public().map_err(GitAiError::Generic)?;
    let mut changed = false;

    if let Some(api_base) = api_base
        && file_config.api_base_url.as_deref() != Some(api_base.as_str())
    {
        file_config.api_base_url = Some(api_base.clone());
        changed = true;
    }

    if let Some(api_key) = api_key
        && file_config.api_key.as_deref() != Some(api_key.as_str())
    {
        file_config.api_key = Some(api_key.clone());
        changed = true;
    }

    if api_base.is_some() {
        let git_path_missing = file_config
            .git_path
            .as_ref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(true);
        if git_path_missing && let Some(git_path) = detect_install_git_path(binary_path) {
            file_config.git_path = Some(git_path);
            changed = true;
        }
    }

    if !changed {
        return Ok(false);
    }

    crate::config::save_file_config(&file_config).map_err(GitAiError::Generic)?;
    Ok(true)
}

pub(super) fn detect_install_git_path(binary_path: &Path) -> Option<String> {
    let install_dir = binary_path.parent()?;

    #[cfg(windows)]
    {
        parse_git_og_cmd_path(&fs::read_to_string(install_dir.join("git-og.cmd")).ok()?)
    }

    #[cfg(not(windows))]
    {
        let target = fs::read_link(install_dir.join("git-og")).ok()?;
        let resolved = if target.is_absolute() {
            target
        } else {
            install_dir.join(target)
        };
        Some(resolved.to_string_lossy().to_string())
    }
}

#[cfg(windows)]
pub(super) fn parse_git_og_cmd_path(contents: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let start = line.find('"')?;
        let rest = &line[start + 1..];
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    })
}
