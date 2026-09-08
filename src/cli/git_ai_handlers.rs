mod checkpoint;
mod help;
mod internal;
use crate::cli::fail::{fail, resolve_repo_in_cwd_or_fail, resolve_repo_or_fail};
use crate::config;
use crate::model::repository::internal_db::InternalDatabase;
use crate::observability::log_message;
use crate::operations::authorship::ignore::effective_ignore_patterns;
use crate::operations::authorship::range_authorship;
use crate::operations::authorship::stats::stats_command;
use crate::operations::commands;
use crate::operations::git::repository::CommitRange;
use crate::process_spawn::is_interactive_terminal;
use checkpoint::handle_checkpoint;
use help::print_help;
pub(crate) use internal::{
    handle_blame_analysis_internal, handle_effective_ignore_patterns_internal,
    handle_fetch_authorship_notes_internal, handle_push_authorship_notes_internal,
};
use std::io::IsTerminal;

pub fn handle_git_ai(args: &[String]) {
    let perf_entry =
        if std::env::var("GIT_AI_DEBUG_PERFORMANCE").is_ok_and(|v| !v.is_empty() && v != "0") {
            Some(std::time::Instant::now())
        } else {
            None
        };

    if args.is_empty() {
        print_help();
        return;
    }

    // Initialize the global telemetry handle so that observability and CAS
    // events are routed over the control socket instead of being written to
    // per-PID log files.
    //
    // Skip for commands that must work without a running background service
    // (help, version, config, d management, debug, upgrade) so users can
    // always diagnose and recover from a broken state.
    let needs_daemon =
        crate::cli::daemon_preconnect::command_requires_daemon_preconnect(args[0].as_str());
    if needs_daemon {
        use crate::operations::daemon::telemetry_handle::{
            DaemonTelemetryInitResult, init_daemon_telemetry_handle,
        };
        match init_daemon_telemetry_handle() {
            DaemonTelemetryInitResult::Connected | DaemonTelemetryInitResult::Skipped => {}
            DaemonTelemetryInitResult::Failed(err) => {
                eprintln!(
                    "error: failed to connect to git-ai background service: {}",
                    err
                );
                if args[0].as_str() == "checkpoint" {
                    std::process::exit(0);
                }
                std::process::exit(1);
            }
        }
    }

    // Start DB warmup early for commands that need database access
    if args[0].as_str() == "show-prompt" {
        InternalDatabase::warmup();
    }

    match args[0].as_str() {
        "help" | "--help" | "-h" => {
            print_help();
        }
        "version" | "--version" | "-v" => {
            if cfg!(debug_assertions) {
                println!("{} (debug)", env!("CARGO_PKG_VERSION"));
            } else {
                println!(env!("CARGO_PKG_VERSION"));
            }
            std::process::exit(0);
        }
        "config" => {
            commands::config::handle_config(&args[1..]);
            if is_interactive_terminal() {
                log_message("config", "info", None)
            }
        }
        "debug" => {
            commands::debug::handle_debug(&args[1..]);
        }
        "bg" | "d" | "daemon" => {
            commands::daemon::handle_daemon(&args[1..]);
        }
        "stats" => {
            if is_interactive_terminal() {
                log_message("stats", "info", None)
            }
            handle_stats(&args[1..]);
        }
        "usage" => {
            commands::usage::handle_usage(&args[1..]);
        }
        "analyze" => {
            commands::analyze::handle_analyze(&args[1..]);
            if is_interactive_terminal() {
                log_message("analyze", "info", None)
            }
        }
        "status" => {
            commands::status::handle_status(&args[1..]);
        }
        "show" => {
            commands::show::handle_show(&args[1..]);
        }
        "checkpoint" => {
            if let Some(t) = perf_entry {
                eprintln!(
                    "[perf] checkpoint: entry_overhead={:.1}ms (binary startup + clap + dispatch)",
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
            handle_checkpoint(&args[1..]);
        }
        "log" => {
            let status = commands::log::handle_log(&args[1..]);
            if is_interactive_terminal() {
                log_message("log", "info", None)
            }
            exit_with_log_status(status);
        }
        "blame" => {
            handle_ai_blame(&args[1..]);
            if is_interactive_terminal() {
                log_message("blame", "info", None)
            }
        }
        "diff" => {
            handle_ai_diff(&args[1..]);
            if is_interactive_terminal() {
                log_message("diff", "info", None)
            }
        }
        "git-path" => {
            let config = config::Config::get();
            println!("{}", config.git_cmd());
            std::process::exit(0);
        }
        "install-hooks" | "install" => match commands::install_hooks::run_cli(&args[1..]) {
            Ok(commands::install_hooks::InstallCommandOutcome::Help) => {
                commands::install_hooks::print_install_help(args[0].as_str());
            }
            Ok(commands::install_hooks::InstallCommandOutcome::Installed(statuses)) => {
                if let Ok(statuses_value) = serde_json::to_value(&statuses) {
                    log_message("install-hooks", "info", Some(statuses_value));
                }
            }
            Err(e) => fail("Install hooks", e),
        },
        "uninstall" => {
            if let Err(e) = commands::uninstall::run_uninstall_all(&args[1..]) {
                fail("Uninstall", e);
            }
        }
        "uninstall-hooks" => match commands::install_hooks::run_uninstall_cli(&args[1..]) {
            Ok(commands::install_hooks::UninstallCommandOutcome::Help) => {
                commands::install_hooks::print_uninstall_help();
            }
            Ok(commands::install_hooks::UninstallCommandOutcome::Uninstalled(statuses)) => {
                if let Ok(statuses_value) = serde_json::to_value(&statuses) {
                    log_message("uninstall-hooks", "info", Some(statuses_value));
                }
            }
            Err(e) => fail("Uninstall hooks", e),
        },
        "git-hooks" => {
            handle_git_hooks(&args[1..]);
        }
        "ci" => {
            commands::ci_handlers::handle_ci(&args[1..]);
        }
        "upgrade" => {
            commands::upgrade::run_with_args(&args[1..]);
        }
        "flush-metrics-db" => {
            commands::flush_metrics_db::handle_flush_metrics_db(&args[1..]);
        }
        "reingest" => {
            commands::reingest::handle_reingest(&args[1..]);
        }
        "await" => {
            commands::r#await::handle_await(&args[1..]);
        }
        "login" => {
            commands::login::handle_login(&args[1..]);
        }
        "logout" => {
            commands::logout::handle_logout(&args[1..]);
        }
        "whoami" => {
            commands::whoami::handle_whoami(&args[1..]);
        }
        "exchange-nonce" => {
            commands::exchange_nonce::handle_exchange_nonce(&args[1..]);
        }
        "dash" | "dashboard" => {
            commands::personal_dashboard::handle_personal_dashboard(&args[1..]);
        }
        "show-prompt" => {
            commands::show_prompt::handle_show_prompt(&args[1..]);
        }
        "fetch-notes" => {
            commands::fetch_notes::handle_fetch_notes(&args[1..]);
        }
        "effective-ignore-patterns" => {
            handle_effective_ignore_patterns_internal(&args[1..]);
        }
        "blame-analysis" => {
            handle_blame_analysis_internal(&args[1..]);
        }
        "fetch-authorship-notes" | "fetch_authorship_notes" => {
            handle_fetch_authorship_notes_internal(&args[1..]);
        }
        "push-authorship-notes" | "push_authorship_notes" => {
            handle_push_authorship_notes_internal(&args[1..]);
        }
        "notes" => {
            handle_notes_subcommand(&args[1..]);
        }
        _ => {
            println!("Unknown git-ai command: {}", args[0]);
            std::process::exit(1);
        }
    }
}

/// Dispatch `git-ai notes <subcommand>` commands.
pub(crate) fn handle_notes_subcommand(args: &[String]) {
    let subcommand = args.first().map(|s| s.as_str()).unwrap_or("--help");
    match subcommand {
        "migrate" => {
            commands::notes_migrate::handle_notes_migrate(&args[1..]);
        }
        // Hidden: in-memory reference implementation of the notes backend HTTP
        // contract. Intentionally not advertised in `--help`; it is for
        // developers, tests, and benchmarks, not end users.
        "serve" => {
            handle_notes_serve(&args[1..]);
        }
        "--help" | "-h" | "help" => {
            eprintln!("git ai notes - Notes backend management commands");
            eprintln!();
            eprintln!("Usage: git ai notes <subcommand> [options]");
            eprintln!();
            eprintln!("Subcommands:");
            eprintln!("  migrate    Bulk-upload existing git notes to the HTTP backend");
            eprintln!();
            eprintln!("Run 'git ai notes <subcommand> --help' for details.");
        }
        other => {
            eprintln!("Unknown git-ai notes subcommand: {}", other);
            eprintln!("Run 'git ai notes --help' for usage.");
            std::process::exit(1);
        }
    }
}

/// `git-ai notes serve` — run the in-memory reference notes backend.
///
/// This is a developer/test tool. The server stores everything in process
/// memory and accepts any auth header. See
/// `crate::notes::reference_server` for the wire contract.
fn handle_notes_serve(args: &[String]) {
    let mut bind: String = "127.0.0.1:0".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--bind" if i + 1 < args.len() => {
                bind = args[i + 1].clone();
                i += 2;
            }
            "--port" if i + 1 < args.len() => {
                bind = format!("127.0.0.1:{}", args[i + 1]);
                i += 2;
            }
            "--help" | "-h" => {
                eprintln!(
                    "git ai notes serve - Run the in-memory notes backend reference server\n\
                     \n\
                     Usage: git ai notes serve [--bind <addr:port>] [--port <port>]\n\
                     \n\
                     This is a reference implementation. All notes are stored in process\n\
                     memory; auth headers are accepted but not validated. It exists to\n\
                     document the HTTP wire contract and to enable local testing of the\n\
                     `notes_backend.kind = http` code path without a real backend."
                );
                return;
            }
            other => {
                eprintln!("Unknown argument to `git ai notes serve`: {}", other);
                std::process::exit(1);
            }
        }
    }

    if let Err(e) = crate::notes::reference_server::run_blocking(&bind) {
        fail("notes reference server", e);
    }
}

fn handle_ai_blame(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: blame requires a file argument");
        std::process::exit(1);
    }

    // Find the git repository from current directory
    let (repo, current_dir) = resolve_repo_in_cwd_or_fail();

    // Parse blame arguments
    let (file_path, mut options) = match commands::blame::parse_blame_args(args) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Failed to parse blame arguments: {}", e);
            std::process::exit(1);
        }
    };

    // Auto-detect ignore-revs-file if not explicitly provided, not disabled via --no-ignore-revs-file,
    // and git version supports --ignore-revs-file (git >= 2.23)
    if options.ignore_revs_file.is_none()
        && !options.no_ignore_revs_file
        && repo.git_supports_ignore_revs_file()
    {
        // First, check git config for blame.ignoreRevsFile
        if let Ok(Some(config_path)) = repo.config_get_str("blame.ignoreRevsFile")
            && !config_path.is_empty()
        {
            // Config path could be relative to repo root or absolute
            if let Ok(workdir) = repo.workdir() {
                let full_path = if std::path::Path::new(&config_path).is_absolute() {
                    std::path::PathBuf::from(&config_path)
                } else {
                    workdir.join(&config_path)
                };
                if full_path.exists() {
                    options.ignore_revs_file = Some(full_path.to_string_lossy().to_string());
                }
            }
        }

        // If still not set, check for .git-blame-ignore-revs in the repository root
        if options.ignore_revs_file.is_none()
            && let Ok(workdir) = repo.workdir()
        {
            let ignore_revs_path = workdir.join(".git-blame-ignore-revs");
            if ignore_revs_path.exists() {
                options.ignore_revs_file = Some(ignore_revs_path.to_string_lossy().to_string());
            }
        }
    }

    // Check if this is an interactive terminal
    let is_interactive = std::io::stdout().is_terminal();

    if is_interactive && options.incremental {
        // For incremental mode in interactive terminal, we need special handling
        // This would typically involve a pager like less
        eprintln!("Error: incremental mode is not supported in interactive terminal");
        std::process::exit(1);
    }

    let file_path = if !std::path::Path::new(&file_path).is_absolute() {
        let current_dir_path = std::path::PathBuf::from(&current_dir);
        current_dir_path
            .join(&file_path)
            .to_string_lossy()
            .to_string()
    } else {
        file_path
    };

    if let Err(e) = repo.blame(&file_path, &options) {
        fail("Blame", e);
    }
}

fn handle_ai_diff(args: &[String]) {
    let (repo, _current_dir) = resolve_repo_in_cwd_or_fail();
    if let Err(e) = commands::diff::handle_diff(&repo, args) {
        fail("Diff", e);
    }
}

fn handle_stats(args: &[String]) {
    // Find the git repository
    let repo = resolve_repo_or_fail();
    // Parse stats-specific arguments
    let mut json_output = false;
    let mut commit_sha = None;
    let mut commit_range: Option<CommitRange> = None;
    let mut ignore_patterns: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => {
                json_output = true;
                i += 1;
            }
            "--ignore" => {
                // Collect all arguments after --ignore until we hit another flag or commit SHA
                // This supports shell glob expansion: `--ignore *.lock` expands to `--ignore Cargo.lock package.lock`
                i += 1;
                let mut found_pattern = false;
                while i < args.len() {
                    let arg = &args[i];
                    // Stop if we hit another flag
                    if arg.starts_with("--") {
                        break;
                    }
                    // Stop if this looks like a commit SHA or range (contains ..)
                    if arg.contains("..")
                        || (commit_sha.is_none() && !found_pattern && arg.len() >= 7)
                    {
                        // Could be a commit SHA, stop collecting patterns
                        break;
                    }
                    ignore_patterns.push(arg.clone());
                    found_pattern = true;
                    i += 1;
                }
                if !found_pattern {
                    eprintln!("--ignore requires at least one pattern argument");
                    std::process::exit(1);
                }
            }
            _ => {
                // First non-flag argument is treated as commit SHA or range
                if commit_sha.is_none() {
                    let arg = &args[i];
                    // Check if this is a commit range (contains "..")
                    if arg.contains("..") {
                        let parts: Vec<&str> = arg.split("..").collect();
                        if parts.len() == 2 {
                            match CommitRange::new_infer_refname(
                                &repo,
                                commands::revision::normalize_head_rev(parts[0]),
                                commands::revision::normalize_head_rev(parts[1]),
                                // @todo this is probably fine, but we might want to give users an option to override from this command.
                                None,
                            ) {
                                Ok(range) => {
                                    commit_range = Some(range);
                                }
                                Err(e) => {
                                    eprintln!("Failed to create commit range: {}", e);
                                    std::process::exit(1);
                                }
                            }
                        } else {
                            eprintln!("Invalid commit range format. Expected: <commit>..<commit>");
                            std::process::exit(1);
                        }
                    } else {
                        commit_sha = Some(commands::revision::normalize_head_rev(arg));
                    }
                    i += 1;
                } else {
                    eprintln!("Unknown stats argument: {}", args[i]);
                    std::process::exit(1);
                }
            }
        }
    }

    let effective_patterns = effective_ignore_patterns(&repo, &ignore_patterns, &[]);

    // Handle commit range if detected
    if let Some(range) = commit_range {
        match range_authorship::range_authorship(range, false, &effective_patterns, None) {
            Ok(stats) => {
                if json_output {
                    let json_str = serde_json::to_string(&stats).unwrap();
                    println!("{}", json_str);
                } else {
                    range_authorship::print_range_authorship_stats(&stats);
                }
            }
            Err(e) => fail("Range authorship", e),
        }
        return;
    }

    if let Err(e) = stats_command(
        &repo,
        commit_sha.as_deref(),
        json_output,
        &effective_patterns,
    ) {
        match e {
            crate::error::GitAiError::Generic(msg) if msg.starts_with("No commit found:") => {
                eprintln!("{}", msg);
            }
            _ => {
                eprintln!("Stats failed: {}", e);
            }
        }
        std::process::exit(1);
    }
}

fn handle_git_hooks(args: &[String]) {
    match args.first().map(String::as_str) {
        Some("remove") | Some("uninstall") => {
            let repo = resolve_repo_or_fail();

            match commands::git_hook_handlers::remove_repo_hooks(&repo, false) {
                Ok(report) => {
                    let status = if report.changed { "removed" } else { "ok" };
                    println!(
                        "repo hooks {}: {}",
                        status,
                        report.managed_hooks_path.to_string_lossy()
                    );
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("Failed to remove repo hooks: {}", e);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("The git core hooks feature has been sunset.");
            eprintln!("Usage: git-ai git-hooks remove");
            std::process::exit(1);
        }
    }
}

/// Synthesize JSON hook_input from CLI args for mock/test presets that can be
/// invoked without --hook-input.
/// Exit mirroring the child's termination status, re-raising the original
/// signal on Unix so the calling shell sees the correct termination reason
/// (e.g. SIGPIPE from `git ai log | head`).
fn exit_with_log_status(status: std::process::ExitStatus) -> ! {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            unsafe {
                libc::signal(sig, libc::SIG_DFL);
                libc::raise(sig);
            }
            unreachable!();
        }
    }
    std::process::exit(status.code().unwrap_or(1));
}
