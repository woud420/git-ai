mod cli;
mod configuration;
mod execution;
mod installer_environment;
use crate::error::GitAiError;
use crate::operations::commands::install_manifest::{InstallManifest, TRACE2_GIT_CONFIG_KEYS};
use crate::operations::mdm::hook_installer::HookInstallerParams;
use crate::operations::mdm::paths::get_current_binary_path;
use cli::{InstallAction, InstallOptions, parse_install_action};
pub(crate) use cli::{
    InstallCommandOutcome, UninstallCommandOutcome, print_install_help, print_uninstall_help,
};
pub(crate) use configuration::remove_global_git_config_section;
use configuration::{
    InstallConfig, configure_daemon_trace2, ensure_daemon, persist_install_config_with_values,
};
use execution::{async_run_install, async_run_uninstall, cleanup_legacy_envelope_logs};
use std::collections::HashMap;

pub(crate) const TRACE2_EVENT_TARGET_KEY: &str = "trace2.eventTarget";
pub(crate) const TRACE2_EVENT_NESTING_KEY: &str = "trace2.eventNesting";
const TRACE2_EVENT_NESTING_VALUE: &str = "0";
const VISUAL_STUDIO_INSTALLER_ID: &str = "visual-studio";

/// Installation status for a tool
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStatus {
    /// Tool was not detected on the machine
    NotFound,
    /// Hooks/extensions were successfully installed or updated
    Installed,
    /// Hooks/extensions were already up to date
    AlreadyInstalled,
    /// Installation attempted but failed
    Failed,
}

impl InstallStatus {
    /// Convert status to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            InstallStatus::NotFound => "not_found",
            InstallStatus::Installed => "installed",
            InstallStatus::AlreadyInstalled => "already_installed",
            InstallStatus::Failed => "failed",
        }
    }
}

/// Detailed install result for metrics tracking
#[derive(Debug, Clone)]
pub struct InstallResult {
    pub status: InstallStatus,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

impl InstallResult {
    pub fn installed() -> Self {
        Self {
            status: InstallStatus::Installed,
            error: None,
            warnings: Vec::new(),
        }
    }

    pub fn already_installed() -> Self {
        Self {
            status: InstallStatus::AlreadyInstalled,
            error: None,
            warnings: Vec::new(),
        }
    }

    pub fn not_found() -> Self {
        Self {
            status: InstallStatus::NotFound,
            error: None,
            warnings: Vec::new(),
        }
    }

    pub fn failed(msg: impl Into<String>) -> Self {
        Self {
            status: InstallStatus::Failed,
            error: Some(msg.into()),
            warnings: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Get message for ClickHouse (error if failed, else joined warnings)
    pub fn message_for_metrics(&self) -> Option<String> {
        if let Some(err) = &self.error {
            Some(err.clone())
        } else if !self.warnings.is_empty() {
            Some(self.warnings.join("; "))
        } else {
            None
        }
    }
}

/// Convert a HashMap of tool statuses to string keys and values
pub fn to_hashmap(statuses: HashMap<String, InstallStatus>) -> HashMap<String, String> {
    statuses
        .into_iter()
        .map(|(k, v)| (k, v.as_str().to_string()))
        .collect()
}

/// Main entry point for install-hooks command
pub fn run(args: &[String]) -> Result<HashMap<String, String>, GitAiError> {
    match run_cli(args)? {
        InstallCommandOutcome::Help => Ok(HashMap::new()),
        InstallCommandOutcome::Installed(statuses) => Ok(statuses),
    }
}

pub(crate) fn run_cli(args: &[String]) -> Result<InstallCommandOutcome, GitAiError> {
    match parse_install_action(args)? {
        InstallAction::Help => Ok(InstallCommandOutcome::Help),
        InstallAction::Install(options) => {
            run_install(options).map(InstallCommandOutcome::Installed)
        }
    }
}

fn run_install(options: InstallOptions) -> Result<HashMap<String, String>, GitAiError> {
    options.installer_environment.apply();
    let install_config = InstallConfig {
        api_base: options.api_base.clone().or_else(|| {
            std::env::var("API_BASE")
                .ok()
                .filter(|value| !value.is_empty())
        }),
        api_key: options.api_key.clone().or_else(|| {
            std::env::var("API_KEY")
                .ok()
                .filter(|value| !value.is_empty())
        }),
    };

    // Daemon trace2 config must be in place before any install work starts.
    // Non-fatal: the global git config may be read-only (e.g. Nix store symlink).
    if let Err(e) = configure_daemon_trace2(options.dry_run) {
        eprintln!("Warning: could not configure trace2 (non-fatal): {e}");
    }
    ensure_daemon(options.dry_run);

    // Now that the daemon is (re)started, initialize the telemetry handle so
    // that install-hooks metrics and observability events route through it.
    if !options.dry_run {
        let _ = crate::operations::daemon::telemetry_handle::init_daemon_telemetry_handle();
    }

    // Get absolute path to the current binary
    let binary_path = get_current_binary_path()?;
    persist_install_config_with_values(&binary_path, options.dry_run, &install_config)?;
    let params = HookInstallerParams { binary_path };

    // Run async operations and convert result.
    let statuses = crate::tokio_runtime::block_on(async_run_install(&params, &options))?;

    let statuses = to_hashmap(statuses);
    // Clean up legacy artifacts and update the install manifest.
    if !options.dry_run {
        cleanup_legacy_envelope_logs();
        InstallManifest::record_install_hooks(TRACE2_GIT_CONFIG_KEYS, &statuses).unwrap_or_else(
            |e| eprintln!("[git-ai] warning: could not write install manifest: {e}"),
        );
    }
    Ok(statuses)
}

/// Main entry point for uninstall-hooks command
pub fn run_uninstall(args: &[String]) -> Result<HashMap<String, String>, GitAiError> {
    match run_uninstall_cli(args)? {
        UninstallCommandOutcome::Help => Ok(HashMap::new()),
        UninstallCommandOutcome::Uninstalled(statuses) => Ok(statuses),
    }
}

pub(crate) fn run_uninstall_cli(args: &[String]) -> Result<UninstallCommandOutcome, GitAiError> {
    let cli::UninstallAction::Uninstall(options) = cli::parse_uninstall_action(args)? else {
        return Ok(UninstallCommandOutcome::Help);
    };

    // Get absolute path to the current binary
    let binary_path = get_current_binary_path()?;
    let params = HookInstallerParams { binary_path };

    // Run async operations and convert result.
    let statuses = crate::tokio_runtime::block_on(async_run_uninstall(
        &params,
        options.dry_run,
        options.verbose,
    ))?;
    Ok(UninstallCommandOutcome::Uninstalled(to_hashmap(statuses)))
}

#[cfg(test)]
mod tests;
