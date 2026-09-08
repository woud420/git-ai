use super::cli::InstallOptions;
use super::configuration::should_include_installer;
use super::{InstallResult, InstallStatus};
use crate::error::GitAiError;
use crate::operations::git::repository::probe_configured_git_version;
use crate::operations::mdm::agents::get_all_installers;
use crate::operations::mdm::hook_installer::HookInstallerParams;
use crate::operations::mdm::paths::home_dir;
use crate::operations::mdm::skills_installer;
use crate::operations::mdm::spinner::{Spinner, print_diff};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::process::{Command, Stdio};

pub(super) fn print_amp_plugins_note(installer_id: &str) {
    if installer_id == "amp" {
        println!("  Note: Amp plugins are experimental. Run amp with `PLUGINS=all amp`.");
    }
}

/// Find PIDs of running processes that match any of the given process names.
/// Returns a list of (pid, process_name) tuples for each match found.
pub(super) fn find_running_pids(process_names: &[&str]) -> Vec<(u32, String)> {
    if process_names.is_empty() {
        return vec![];
    }

    let output = {
        #[cfg(unix)]
        {
            Command::new("ps")
                .args(["axo", "pid,comm"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
        }
        #[cfg(windows)]
        {
            Command::new("tasklist")
                .args(["/FO", "CSV", "/NH"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
        }
    };

    let Ok(output) = output else {
        return vec![];
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results: Vec<(u32, String)> = Vec::new();

    for line in stdout.lines() {
        #[cfg(unix)]
        {
            let trimmed = line.trim();
            // ps output: "  PID COMM" — split on whitespace
            let mut parts = trimmed.splitn(2, char::is_whitespace);
            let pid_str = parts.next().unwrap_or("").trim();
            let comm = parts.next().unwrap_or("").trim();
            // comm may be a full path; extract the basename
            let base = comm.rsplit('/').next().unwrap_or(comm);
            if let Ok(pid) = pid_str.parse::<u32>() {
                for &name in process_names {
                    if base.eq_ignore_ascii_case(name) {
                        results.push((pid, base.to_string()));
                        break;
                    }
                }
            }
        }
        #[cfg(windows)]
        {
            // tasklist CSV: "Image Name","PID",...
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() >= 2 {
                let image = fields[0].trim_matches('"');
                let pid_str = fields[1].trim_matches('"');
                let base = image.strip_suffix(".exe").unwrap_or(image);
                if let Ok(pid) = pid_str.parse::<u32>() {
                    for &name in process_names {
                        if base.eq_ignore_ascii_case(name) {
                            results.push((pid, base.to_string()));
                            break;
                        }
                    }
                }
            }
        }
    }

    results
}

pub(super) async fn async_run_install(
    params: &HookInstallerParams,
    options: &InstallOptions,
) -> Result<HashMap<String, InstallStatus>, GitAiError> {
    let mut any_checked = false;
    let mut has_changes = false;
    let mut statuses: HashMap<String, InstallStatus> = HashMap::new();
    // Track detailed results for metrics (tool_id, result)
    let mut detailed_results: Vec<(String, InstallResult)> = Vec::new();

    // === Coding Agents ===
    println!("\n\x1b[1mCoding Agents\x1b[0m");

    let installers = get_all_installers();
    let mut installed_tools: HashSet<String> = HashSet::new();
    // Track agents whose hooks were updated (name, process_names) for restart warnings
    let mut updated_agents: Vec<(String, Vec<String>)> = Vec::new();

    for installer in &installers {
        let name = installer.name();
        let id = installer.id();

        if !should_include_installer(id, options) {
            continue;
        }

        // Check if tool is installed and hooks status
        match installer.check_hooks(params) {
            Ok(check_result) => {
                if !check_result.tool_installed {
                    statuses.insert(id.to_string(), InstallStatus::NotFound);
                    detailed_results.push((id.to_string(), InstallResult::not_found()));
                    continue;
                }

                installed_tools.insert(id.to_string());
                any_checked = true;

                // Install/update hooks (only for tools that use config file hooks)
                if installer.uses_config_hooks() {
                    let spinner = Spinner::new(&format!("{}: checking hooks", name));
                    spinner.start();

                    match installer.install_hooks(params, options.dry_run) {
                        Ok(Some(diff)) => {
                            if options.dry_run {
                                spinner.pending(&format!("{}: Pending updates", name));
                            } else {
                                spinner.success(&format!("{}: Hooks updated", name));
                                print_amp_plugins_note(id);
                            }
                            if options.verbose {
                                println!();
                                print_diff(&diff);
                            }
                            has_changes = true;
                            statuses.insert(id.to_string(), InstallStatus::Installed);
                            detailed_results.push((id.to_string(), InstallResult::installed()));

                            // Track this agent for restart detection (skip in dry-run)
                            if !options.dry_run {
                                let pnames: Vec<String> = installer
                                    .process_names()
                                    .iter()
                                    .map(|s| s.to_string())
                                    .collect();
                                if !pnames.is_empty() {
                                    updated_agents.push((name.to_string(), pnames));
                                }
                            }
                        }
                        Ok(None) => {
                            spinner.success(&format!("{}: Hooks already up to date", name));
                            print_amp_plugins_note(id);
                            statuses.insert(id.to_string(), InstallStatus::AlreadyInstalled);
                            detailed_results
                                .push((id.to_string(), InstallResult::already_installed()));
                        }
                        Err(e) => {
                            let error_msg = e.to_string();
                            spinner.error(&format!("{}: Failed to update hooks", name));
                            eprintln!("  Error: {}", error_msg);
                            statuses.insert(id.to_string(), InstallStatus::Failed);
                            detailed_results
                                .push((id.to_string(), InstallResult::failed(error_msg)));
                        }
                    }
                }

                // Install extras (extensions, git.path, etc.)
                match installer.install_extras(params, options.dry_run) {
                    Ok(results) => {
                        let mut extras_changed = false;
                        for result in results {
                            if result.changed {
                                has_changes = true;
                                extras_changed = true;
                            }
                            if result.changed && !options.dry_run {
                                let extra_spinner = Spinner::new(&result.message);
                                extra_spinner.start();
                                extra_spinner.success(&result.message);
                            } else if result.changed && options.dry_run {
                                let extra_spinner = Spinner::new(&result.message);
                                extra_spinner.start();
                                extra_spinner.pending(&result.message);
                            } else if result.message.contains("already") {
                                let extra_spinner = Spinner::new(&result.message);
                                extra_spinner.start();
                                extra_spinner.success(&result.message);
                            } else if result.message.contains("Unable")
                                || result.message.contains("manually")
                            {
                                let extra_spinner = Spinner::new(&result.message);
                                extra_spinner.start();
                                extra_spinner.pending(&result.message);
                            }
                            if options.verbose
                                && let Some(diff) = result.diff
                            {
                                println!();
                                print_diff(&diff);
                            }

                            // Capture warning-like messages for metrics
                            if (result.message.contains("Unable")
                                || result.message.contains("manually")
                                || result.message.contains("Failed"))
                                && let Some((_, detail)) = detailed_results
                                    .iter_mut()
                                    .find(|(tool_id, _)| tool_id == id)
                            {
                                detail.warnings.push(result.message.clone());
                            }
                        }

                        // Track restart detection for extras-only agents (e.g. JetBrains, VS Code)
                        if extras_changed
                            && !options.dry_run
                            && !updated_agents.iter().any(|(n, _)| n == name)
                        {
                            let pnames: Vec<String> = installer
                                .process_names()
                                .iter()
                                .map(|s| s.to_string())
                                .collect();
                            if !pnames.is_empty() {
                                updated_agents.push((name.to_string(), pnames));
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  Error installing extras for {}: {}", name, e);
                        // Capture extras error as a warning on the tool's result
                        if let Some((_, detail)) = detailed_results
                            .iter_mut()
                            .find(|(tool_id, _)| tool_id == id)
                        {
                            detail.warnings.push(format!("Extras install error: {}", e));
                        }
                    }
                }
            }
            Err(check_error) => {
                let error_msg = check_error.to_string();
                any_checked = true;
                let spinner = Spinner::new(&format!("{}: checking hooks", name));
                spinner.start();
                spinner.error(&format!("{}: Hook check failed", name));
                eprintln!("  Error: {}", error_msg);
                statuses.insert(id.to_string(), InstallStatus::Failed);
                detailed_results.push((id.to_string(), InstallResult::failed(error_msg)));
            }
        }
    }

    if options.install_skills {
        if let Ok(result) =
            skills_installer::install_skills(options.dry_run, options.verbose, &installed_tools)
            && result.changed
        {
            has_changes = true;
        }
    } else if let Ok(result) = skills_installer::uninstall_skills(options.dry_run, options.verbose)
        && result.changed
    {
        has_changes = true;
    }

    if !any_checked {
        println!("No compatible IDEs or agent configurations detected. Nothing to install.");
    } else if has_changes && options.dry_run {
        println!("\n\x1b[33m⚠ Dry-run mode (default). No changes were made.\x1b[0m");
        println!("To apply these changes, run:");
        println!("\x1b[1m  git-ai install-hooks --dry-run=false\x1b[0m");
    }

    // Check for running agents that had hooks updated and warn about restart
    if !options.dry_run && !updated_agents.is_empty() {
        let mut any_running = false;

        for (agent_name, pnames) in &updated_agents {
            let refs: Vec<&str> = pnames.iter().map(|s| s.as_str()).collect();
            let pids = find_running_pids(&refs);
            if !pids.is_empty() {
                if !any_running {
                    println!(
                        "\n\x1b[33m⚠ The following agents are currently running and must be restarted:\x1b[0m"
                    );
                    any_running = true;
                }
                let pid_list: Vec<String> = pids.iter().map(|(pid, _)| pid.to_string()).collect();
                println!(
                    "  \x1b[1m{}\x1b[0m (PID: {})",
                    agent_name,
                    pid_list.join(", ")
                );
            }
        }

        if any_running {
            println!();
            println!(
                "\x1b[33mRestart the agents listed above for git-ai attribution to take effect.\x1b[0m"
            );
            println!(
                "Any work done before installing git-ai (or before restarting) will be attributed as human."
            );
            println!(
                "This is expected — once you commit and start a fresh session, attribution will work correctly."
            );
            println!(
                "If the issue persists, please open an issue at https://github.com/git-ai-project/git-ai/issues"
            );
        }
    }

    // Emit metrics for each agent/git_client result (only if not dry-run)
    if !options.dry_run {
        emit_install_hooks_metrics(&detailed_results);
    }

    // Warn if git version is below the minimum required for full functionality
    warn_if_git_version_too_old();

    Ok(statuses)
}

/// Minimum git version required for git-ai to function correctly.
/// git 2.22.0 introduced `git worktree list --porcelain` output format improvements
/// and trace2 event logging used by git-ai for attribution.
pub(super) const MIN_GIT_VERSION: (u32, u32, u32) = (2, 22, 0);

/// Print a loud warning if the installed git version is older than MIN_GIT_VERSION.
pub(super) fn warn_if_git_version_too_old() {
    let version = probe_configured_git_version();

    if let Some(v) = version {
        let (maj, min, patch) = MIN_GIT_VERSION;
        if v < (maj, min, patch) {
            let (vmaj, vmin, vpatch) = v;
            eprintln!();
            eprintln!(
                "\x1b[1;31m╔══════════════════════════════════════════════════════════════╗\x1b[0m"
            );
            eprintln!(
                "\x1b[1;31m║  WARNING: git version too old — git-ai will not work         ║\x1b[0m"
            );
            eprintln!(
                "\x1b[1;31m╚══════════════════════════════════════════════════════════════╝\x1b[0m"
            );
            eprintln!(
                "\x1b[1;31mDetected git {}.{}.{} — git-ai requires git >= {}.{}.{}\x1b[0m",
                vmaj, vmin, vpatch, maj, min, patch
            );
            eprintln!("\x1b[33mPlease upgrade git before using git-ai:\x1b[0m");
            eprintln!("  macOS:   brew install git");
            eprintln!(
                "  Ubuntu:  sudo add-apt-repository ppa:git-core/ppa && sudo apt-get update && sudo apt-get install git"
            );
            eprintln!("  Windows: https://git-scm.com/download/win");
            eprintln!();
        }
    }
}

/// Emit metrics events for install-hooks results
pub(super) fn emit_install_hooks_metrics(results: &[(String, InstallResult)]) {
    use crate::metrics::{EventAttributes, InstallHooksValues};

    let attrs = EventAttributes::with_version(env!("CARGO_PKG_VERSION"));

    for (tool_id, result) in results {
        let mut values = InstallHooksValues::new()
            .tool_id(tool_id.clone())
            .status(result.status.as_str().to_string());

        if let Some(msg) = result.message_for_metrics() {
            values = values.message(msg);
        } else {
            values = values.message_null();
        }

        crate::metrics::record(values, attrs.clone());
    }
}

pub(super) async fn async_run_uninstall(
    params: &HookInstallerParams,
    dry_run: bool,
    verbose: bool,
) -> Result<HashMap<String, InstallStatus>, GitAiError> {
    let mut any_checked = false;
    let mut has_changes = false;
    let mut statuses: HashMap<String, InstallStatus> = HashMap::new();

    // Uninstall skills first (these are global, not per-agent, silently)
    if let Ok(result) = skills_installer::uninstall_skills(dry_run, verbose) {
        if result.changed {
            has_changes = true;
            statuses.insert("skills".to_string(), InstallStatus::Installed);
        } else {
            statuses.insert("skills".to_string(), InstallStatus::AlreadyInstalled);
        }
    }

    // === Coding Agents ===
    println!("\n\x1b[1mCoding Agents\x1b[0m");

    let installers = get_all_installers();

    for installer in installers {
        let name = installer.name();
        let id = installer.id();

        // Check if tool is installed
        match installer.check_hooks(params) {
            Ok(check_result) => {
                if !check_result.tool_installed {
                    statuses.insert(id.to_string(), InstallStatus::NotFound);
                    continue;
                }

                if !check_result.hooks_installed {
                    statuses.insert(id.to_string(), InstallStatus::NotFound);
                    continue;
                }

                any_checked = true;

                // Uninstall hooks
                let spinner = Spinner::new(&format!("{}: removing hooks", name));
                spinner.start();

                match installer.uninstall_hooks(params, dry_run) {
                    Ok(Some(diff)) => {
                        if dry_run {
                            spinner.pending(&format!("{}: Pending removal", name));
                        } else {
                            spinner.success(&format!("{}: Hooks removed", name));
                        }
                        if verbose {
                            println!();
                            print_diff(&diff);
                        }
                        has_changes = true;
                        statuses.insert(id.to_string(), InstallStatus::Installed);
                    }
                    Ok(None) => {
                        spinner.success(&format!("{}: No hooks to remove", name));
                        statuses.insert(id.to_string(), InstallStatus::AlreadyInstalled);
                    }
                    Err(e) => {
                        spinner.error(&format!("{}: Failed to remove hooks", name));
                        eprintln!("  Error: {}", e);
                        statuses.insert(id.to_string(), InstallStatus::Failed);
                    }
                }

                // Uninstall extras
                match installer.uninstall_extras(params, dry_run) {
                    Ok(results) => {
                        for result in results {
                            if result.changed {
                                has_changes = true;
                            }
                            if !result.message.is_empty() {
                                let extra_spinner = Spinner::new(&result.message);
                                extra_spinner.start();
                                if result.changed {
                                    extra_spinner.success(&result.message);
                                } else {
                                    extra_spinner.pending(&result.message);
                                }
                            }
                            if verbose && let Some(diff) = result.diff {
                                println!();
                                print_diff(&diff);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  Error uninstalling extras for {}: {}", name, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("  Error checking {}: {}", name, e);
                statuses.insert(id.to_string(), InstallStatus::Failed);
            }
        }
    }

    if !any_checked {
        println!("No git-ai hooks found to uninstall.");
    } else if has_changes && dry_run {
        println!("\n\x1b[33m⚠ Dry-run mode (default). No changes were made.\x1b[0m");
        println!("To apply these changes, run:");
        println!("\x1b[1m  git-ai uninstall-hooks --dry-run=false\x1b[0m");
    } else if !has_changes {
        println!("All git-ai hooks have been removed.");
    }

    Ok(statuses)
}

/// Remove the legacy envelope logs directory and related lock/marker files.
///
/// All telemetry now flows through the daemon control socket, so the per-PID
/// log file system under `~/.git-ai/internal/logs/` is no longer needed.
pub(super) fn cleanup_legacy_envelope_logs() {
    let internal = home_dir().join(".git-ai").join("internal");

    // Remove the entire logs directory
    let logs_dir = internal.join("logs");
    if logs_dir.is_dir() {
        let _ = fs::remove_dir_all(&logs_dir);
    }

    // Remove the flush-logs lock file
    let _ = fs::remove_file(internal.join("flush-logs.lock"));

    // Remove the debounce marker file
    let _ = fs::remove_file(internal.join("last_flush_trigger_ts"));
}
