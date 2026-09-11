use crate::operations::daemon::self_check::prepare_daemon_for_debug_self_checks;
use diagnostics::MIN_GIT_VERSION_DISPLAY;
mod context;
mod diagnostics;
mod help;
mod jj;
mod models;
use crate::clients::auth::{AuthState, collect_auth_status, format_unix_timestamp};
use crate::config;
use diagnostics::{
    append_git_committer_identity, append_git_diagnostics, append_git_version_check,
    append_indented_block, collect_git_committer_identity_info, collect_git_diagnostics,
    debug_progress, run_command_capture, run_git_command_capture,
};
use diagnostics::{
    collect_git_ai_config_dump, collect_git_config_dump, collect_git_environment,
    collect_hardware_info, collect_platform_info, collect_repository_info, format_bytes,
};
use help::print_debug_help;
use models::{DebugOptions, SKIP_TRACE2_CHECKS_FLAG, ShellGitLookup};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub fn handle_debug(args: &[String]) {
    if args.first().is_some_and(|arg| arg == "jj") {
        std::process::exit(jj::handle(&args[1..]));
    }
    if args.first().is_some_and(|arg| arg == "context") {
        std::process::exit(context::handle(&args[1..]));
    }
    if args
        .iter()
        .any(|arg| arg == "--help" || arg == "-h" || arg == "help")
    {
        print_debug_help();
        std::process::exit(0);
    }

    let options = match parse_debug_options(args) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("Error: {}", err);
            print_debug_help();
            std::process::exit(1);
        }
    };

    let report = build_debug_report(options);
    println!("{}", report);
}

fn parse_debug_options(args: &[String]) -> Result<DebugOptions, String> {
    let mut options = DebugOptions::default();
    for arg in args {
        match arg.as_str() {
            SKIP_TRACE2_CHECKS_FLAG => options.skip_trace2_checks = true,
            unknown => return Err(format!("unknown debug argument: {}", unknown)),
        }
    }
    Ok(options)
}

fn build_debug_report(options: DebugOptions) -> String {
    debug_progress("starting debug report");
    let config = config::Config::get();
    let git_cmd = config.git_cmd().to_string();
    debug_progress("resolving configured and shell git paths");
    let git_cmd_realpath = realpath_for_display(&git_cmd);
    let shell_git_lookup = collect_shell_git_lookup();
    debug_progress("checking daemon readiness");
    let daemon_diagnostics = prepare_daemon_for_debug_self_checks(&git_cmd);
    debug_progress(format!(
        "daemon readiness check {}",
        daemon_diagnostics.status.as_str()
    ));
    debug_progress("running git self-checks");
    let git_diagnostics = collect_git_diagnostics(&git_cmd, options);
    debug_progress("collecting system and configuration details");
    debug_progress("checking git versions");
    let git_version = run_git_command_capture(&git_cmd, &["--version"]);
    let shell_git_version = run_git_command_capture("git", &["--version"]);
    debug_progress("collecting git config");
    let git_config = collect_git_config_dump(&git_cmd);
    debug_progress("collecting git-ai config and login state");
    let git_ai_config = collect_git_ai_config_dump();
    let platform_info = collect_platform_info();
    let hardware_info = collect_hardware_info();
    let repository_info = collect_repository_info();
    let git_committer_identity = collect_git_committer_identity_info(&repository_info);
    let auth_info = collect_auth_status();
    let git_environment = collect_git_environment();
    debug_progress("debug report ready");

    let mut out = String::new();
    let _ = writeln!(out, "git-ai debug report");
    let _ = writeln!(out, "Generated (UTC): {}", chrono::Utc::now().to_rfc3339());
    let _ = writeln!(out);

    let _ = writeln!(out, "== Versions ==");
    let _ = writeln!(
        out,
        "Git AI version: {}",
        if cfg!(debug_assertions) {
            format!("{} (debug)", env!("CARGO_PKG_VERSION"))
        } else {
            env!("CARGO_PKG_VERSION").to_string()
        }
    );
    let _ = writeln!(
        out,
        "Git AI binary: {}",
        env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|e| format!("<unavailable: {}>", e))
    );
    let _ = writeln!(out, "Git binary path: {}", git_cmd);
    let _ = writeln!(out, "Git binary realpath: {}", git_cmd_realpath);
    let _ = writeln!(
        out,
        "Shell git lookup command: {}",
        shell_git_lookup.command
    );
    match shell_git_lookup.path {
        Ok(ref path) => {
            let _ = writeln!(out, "Shell git path: {}", path);
            let _ = writeln!(out, "Shell git realpath: {}", realpath_for_display(path));
        }
        Err(ref err) => {
            let _ = writeln!(out, "Shell git path: <error: {}>", err);
            let _ = writeln!(out, "Shell git realpath: <unavailable>");
        }
    }
    match &git_version {
        Ok(version) => {
            let _ = writeln!(out, "Git version: {}", version);
            append_git_version_check(&mut out, "Git version check", version);
        }
        Err(err) => {
            let _ = writeln!(out, "Git version: <error: {}>", err);
            let _ = writeln!(
                out,
                "Git version check: <error: unable to verify minimum version {}>",
                MIN_GIT_VERSION_DISPLAY
            );
        }
    }
    match &shell_git_version {
        Ok(version) => {
            let _ = writeln!(out, "Shell git version: {}", version);
            append_git_version_check(&mut out, "Shell git version check", version);
        }
        Err(err) => {
            let _ = writeln!(out, "Shell git version: <error: {}>", err);
            let _ = writeln!(
                out,
                "Shell git version check: <error: unable to verify minimum version {}>",
                MIN_GIT_VERSION_DISPLAY
            );
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "== Platform ==");
    let _ = writeln!(out, "OS family: {}", env::consts::FAMILY);
    let _ = writeln!(out, "OS: {}", env::consts::OS);
    let _ = writeln!(out, "Arch: {}", env::consts::ARCH);
    if let Some(kernel) = platform_info.kernel {
        let _ = writeln!(out, "Kernel: {}", kernel);
    } else {
        let _ = writeln!(out, "Kernel: <unavailable>");
    }
    if let Some(hostname) = platform_info.hostname {
        let _ = writeln!(out, "Hostname: {}", hostname);
    } else {
        let _ = writeln!(out, "Hostname: <unavailable>");
    }
    let _ = writeln!(
        out,
        "Shell: {}",
        env::var("SHELL")
            .or_else(|_| env::var("ComSpec"))
            .unwrap_or_else(|_| "<unavailable>".to_string())
    );
    let _ = writeln!(
        out,
        "Current dir: {}",
        env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|e| format!("<unavailable: {}>", e))
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "== Hardware ==");
    match hardware_info.cpu_model {
        Some(cpu) => {
            let _ = writeln!(out, "CPU: {}", cpu);
        }
        None => {
            let _ = writeln!(out, "CPU: <unavailable>");
        }
    }
    match hardware_info.physical_cores {
        Some(cores) => {
            let _ = writeln!(out, "Physical cores: {}", cores);
        }
        None => {
            let _ = writeln!(out, "Physical cores: <unavailable>");
        }
    }
    match hardware_info.logical_cores {
        Some(cores) => {
            let _ = writeln!(out, "Logical cores: {}", cores);
        }
        None => {
            let _ = writeln!(out, "Logical cores: <unavailable>");
        }
    }
    match hardware_info.total_memory_bytes {
        Some(bytes) => {
            let _ = writeln!(out, "Memory: {}", format_bytes(bytes));
        }
        None => {
            let _ = writeln!(out, "Memory: <unavailable>");
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "== Repository ==");
    let _ = writeln!(out, "In repository: {}", repository_info.in_repository);
    if let Some(err) = repository_info.error {
        let _ = writeln!(out, "Repository detection: {}", err);
    } else {
        if let Some(workdir) = repository_info.workdir {
            let _ = writeln!(out, "Workdir: {}", workdir);
        }
        if let Some(git_dir) = repository_info.git_dir {
            let _ = writeln!(out, "Git dir: {}", git_dir);
        }
        if let Some(common_dir) = repository_info.common_dir {
            let _ = writeln!(out, "Git common dir: {}", common_dir);
        }
        if let Some(branch) = repository_info.branch {
            let _ = writeln!(out, "Branch: {}", branch);
        }
        if let Some(head) = repository_info.head {
            let _ = writeln!(out, "HEAD: {}", head);
        }
        if let Some(hooks_path) = repository_info.hooks_path {
            let _ = writeln!(out, "core.hooksPath: {}", hooks_path);
        }
        if !repository_info.remotes.is_empty() {
            let _ = writeln!(out, "Remotes:");
            for (name, url) in repository_info.remotes {
                let _ = writeln!(out, "  {} = {}", name, url);
            }
        }
    }
    let _ = writeln!(out);

    append_git_committer_identity(&mut out, &git_committer_identity);
    let _ = writeln!(out);

    append_git_diagnostics(&mut out, &daemon_diagnostics, &git_diagnostics);
    super::checkpoint_outbox_debug::append_checkpoint_outbox_debug(&mut out);
    let _ = writeln!(out);
    let _ = writeln!(out, "== Git Config ==");
    let _ = writeln!(out, "Command: {}", git_config.command);
    match git_config.output {
        Ok(config_output) => {
            append_indented_block(&mut out, &config_output);
        }
        Err(err) => {
            let _ = writeln!(out, "  <error: {}>", err);
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "== Git AI Config ==");
    match git_ai_config {
        Ok(config_output) => {
            append_indented_block(&mut out, &config_output);
        }
        Err(err) => {
            let _ = writeln!(out, "  <error: {}>", err);
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "== Git AI Login ==");
    let _ = writeln!(out, "Credential backend: {}", auth_info.backend);
    match &auth_info.state {
        AuthState::LoggedOut => {
            let _ = writeln!(out, "Status: logged out");
        }
        AuthState::LoggedIn => {
            let _ = writeln!(out, "Status: logged in");
        }
        AuthState::RefreshExpired => {
            let _ = writeln!(out, "Status: credentials expired (refresh token expired)");
        }
        AuthState::Error(err) => {
            let _ = writeln!(out, "Status: <error: {}>", err);
        }
    }
    if let Some(expires_at) = auth_info.access_token_expires_at {
        let _ = writeln!(
            out,
            "Access token expires at: {}",
            format_unix_timestamp(expires_at)
        );
    }
    if let Some(expires_at) = auth_info.refresh_token_expires_at {
        let _ = writeln!(
            out,
            "Refresh token expires at: {}",
            format_unix_timestamp(expires_at)
        );
    }
    if let Some(user_id) = auth_info.user_id {
        let _ = writeln!(out, "User ID: {}", user_id);
    } else if matches!(
        &auth_info.state,
        AuthState::LoggedIn | AuthState::RefreshExpired
    ) {
        let _ = writeln!(out, "User ID: <unavailable>");
    }
    if let Some(email) = auth_info.email {
        let _ = writeln!(out, "Email: {}", email);
    } else if matches!(
        &auth_info.state,
        AuthState::LoggedIn | AuthState::RefreshExpired
    ) {
        let _ = writeln!(out, "Email: <unavailable>");
    }
    if let Some(name) = auth_info.name {
        let _ = writeln!(out, "Name: {}", name);
    } else if matches!(
        &auth_info.state,
        AuthState::LoggedIn | AuthState::RefreshExpired
    ) {
        let _ = writeln!(out, "Name: <unavailable>");
    }
    if let Some(personal_org_id) = auth_info.personal_org_id {
        let _ = writeln!(out, "Personal org ID: {}", personal_org_id);
    }
    if !auth_info.orgs.is_empty() {
        let _ = writeln!(out, "Organizations:");
        for org in auth_info.orgs {
            let org_id = org.org_id.unwrap_or_else(|| "<unknown-id>".to_string());
            let org_slug = org.org_slug.unwrap_or_else(|| "<unknown-slug>".to_string());
            let org_name = org.org_name.unwrap_or_else(|| "<unknown-name>".to_string());
            let role = org.role.unwrap_or_else(|| "<unknown-role>".to_string());
            let _ = writeln!(
                out,
                "  - {} ({}) [{}] role={}",
                org_slug, org_name, org_id, role
            );
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "== Git Environment ==");
    if git_environment.is_empty() {
        let _ = writeln!(
            out,
            "No GIT_AI_*, GITAI_*, or GIT_* environment variables are set."
        );
    } else {
        let _ = writeln!(out, "GIT_AI_*, GITAI_*, and GIT_* variables set:");
        for entry in git_environment {
            let _ = writeln!(out, "  {}", entry);
        }
    }

    out
}

fn collect_shell_git_lookup() -> ShellGitLookup {
    #[cfg(windows)]
    {
        collect_windows_shell_git_lookup()
    }

    #[cfg(not(windows))]
    {
        collect_unix_shell_git_lookup()
    }
}

#[cfg(not(windows))]
fn collect_unix_shell_git_lookup() -> ShellGitLookup {
    let shell = env::var("SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "sh".to_string());
    let command = format!("{} -lc 'which git'", shell);
    let path = run_command_capture(&shell, &["-lc", "which git"])
        .and_then(|output| select_lookup_path(&output));

    ShellGitLookup { command, path }
}

#[cfg(windows)]
fn collect_windows_shell_git_lookup() -> ShellGitLookup {
    let comspec = env::var("ComSpec")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "cmd.exe".to_string());
    let command = format!("{} /C where git", comspec);
    let path = run_command_capture(&comspec, &["/C", "where git"])
        .and_then(|output| select_lookup_path(&output));

    ShellGitLookup { command, path }
}

fn select_lookup_path(output: &str) -> Result<String, String> {
    let mut first_non_empty = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if first_non_empty.is_none() {
            first_non_empty = Some(trimmed.to_string());
        }

        if Path::new(trimmed).exists() {
            return Ok(trimmed.to_string());
        }
    }

    first_non_empty.ok_or_else(|| "empty output".to_string())
}

fn realpath_for_display(path: &str) -> String {
    fs::canonicalize(path)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|e| format!("<error: {}>", e))
}

#[cfg(test)]
mod tests;
