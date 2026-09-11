use super::models::{
    DebugOptions, GitCommitterIdentityInfo, GitConfigDump, GitDebugDiagnostics, GitVersion,
    HardwareInfo, PlatformInfo, RepositoryCommitterIdentity, RepositoryInfo,
    SKIP_TRACE2_CHECKS_FLAG,
};
use crate::config;
use crate::operations::daemon::attribution_self_check::run_attribution_self_check;
use crate::operations::daemon::self_check::{
    DiagnosticCheckResult, DiagnosticStatus, GitDiagnosticTarget,
};
use crate::operations::git::find_repository_in_path;
use crate::operations::git::repository::{
    GitAuthorIdentity, GitConfigIdentityResolution, global_git_config_identity_resolution,
};
use crate::operations::git::trace2_validation::{
    check_trace2_global_config, run_trace2_file_self_check,
};
use crate::process_spawn::format_posix_shell_command as format_command_for_error;
use crate::process_timeout::{TimedCommandOutput, run_command_with_timeout_and_env};
use std::env;
use std::fmt::Write as _;
#[cfg(all(unix, not(target_os = "macos")))]
use std::fs;
use std::time::Duration;

pub(super) fn collect_git_committer_identity_info(
    repository_info: &RepositoryInfo,
) -> GitCommitterIdentityInfo {
    let global_config = global_git_config_identity_resolution().map_err(|e| e.to_string());
    let author_config = config::Config::fresh_author_cached();
    let repository = repository_info
        .committer_identity
        .clone()
        .map(RepositoryCommitterIdentity::InRepository)
        .unwrap_or_else(|| {
            RepositoryCommitterIdentity::NotInRepository(
                repository_info
                    .error
                    .clone()
                    .unwrap_or_else(|| "not in repository".to_string()),
            )
        });

    GitCommitterIdentityInfo {
        global_config,
        repository,
        author_config,
    }
}

pub(super) fn append_git_committer_identity(out: &mut String, identity: &GitCommitterIdentityInfo) {
    let _ = writeln!(out, "== Git Committer Identity ==");
    let _ = writeln!(out, "Global git config identity:");
    match &identity.global_config {
        Ok(global) => {
            append_raw_git_config_identity(out, global, "  ");
            append_git_author_identity(out, &global.identity, "  ");
        }
        Err(err) => {
            let _ = writeln!(out, "  <error: {}>", err);
        }
    }

    let _ = writeln!(out, "Repository effective committer identity:");
    match &identity.repository {
        RepositoryCommitterIdentity::InRepository(resolution) => {
            let raw = resolution
                .raw_git_var
                .as_deref()
                .unwrap_or("<unavailable; git-ai used config fallback>");
            let _ = writeln!(out, "  Raw GIT_COMMITTER_IDENT: {}", raw);
            append_git_author_identity(out, &resolution.identity, "  ");
        }
        RepositoryCommitterIdentity::NotInRepository(err) => {
            let _ = writeln!(out, "  <not in repository: {}>", err);
        }
    }

    let _ = writeln!(out, "Git AI author config override:");
    append_author_config(out, &identity.author_config, "  ");

    let _ = writeln!(out, "Git AI effective author identity:");
    match &identity.repository {
        RepositoryCommitterIdentity::InRepository(resolution) => {
            let effective_author = resolution
                .identity
                .with_author_config(&identity.author_config);
            append_git_author_identity(out, &effective_author, "  ");
        }
        RepositoryCommitterIdentity::NotInRepository(err) => {
            let _ = writeln!(out, "  <not in repository: {}>", err);
        }
    }
}

pub(super) fn append_raw_git_config_identity(
    out: &mut String,
    identity: &GitConfigIdentityResolution,
    prefix: &str,
) {
    let raw_name = identity.raw_name.as_deref().unwrap_or("<unset>");
    let raw_email = identity.raw_email.as_deref().unwrap_or("<unset>");

    let _ = writeln!(out, "{}Raw user.name: {}", prefix, raw_name);
    let _ = writeln!(out, "{}Raw user.email: {}", prefix, raw_email);
}

pub(super) fn append_git_author_identity(
    out: &mut String,
    identity: &GitAuthorIdentity,
    prefix: &str,
) {
    let formatted = identity
        .formatted()
        .unwrap_or_else(|| "<unavailable>".to_string());
    let name = identity.name.as_deref().unwrap_or("<unavailable>");
    let email = identity.email.as_deref().unwrap_or("<unavailable>");

    let _ = writeln!(out, "{}Formatted: {}", prefix, formatted);
    let _ = writeln!(out, "{}Parsed name: {}", prefix, name);
    let _ = writeln!(out, "{}Parsed email: {}", prefix, email);
}

pub(super) fn append_author_config(out: &mut String, author: &config::AuthorConfig, prefix: &str) {
    let name = author.name.as_deref().unwrap_or("<unset>");
    let email = author.email.as_deref().unwrap_or("<unset>");

    let _ = writeln!(out, "{}author.name: {}", prefix, name);
    let _ = writeln!(out, "{}author.email: {}", prefix, email);
}

pub(super) fn collect_git_diagnostics(
    configured_git: &str,
    options: DebugOptions,
) -> Vec<GitDebugDiagnostics> {
    let targets = vec![
        GitDiagnosticTarget::new("configured git", configured_git),
        GitDiagnosticTarget::new("terminal git", "git"),
    ];

    let trace2_configs: Vec<_> = if options.skip_trace2_checks {
        debug_progress(format!(
            "skipping Trace2 config checks ({})",
            SKIP_TRACE2_CHECKS_FLAG
        ));
        targets
            .iter()
            .map(|_| skipped_trace2_check("trace2 global config check skipped"))
            .collect()
    } else {
        targets
            .iter()
            .map(|target| {
                debug_progress(format!("checking Trace2 config for {}", target.label));
                let result = check_trace2_global_config(target);
                debug_progress(format!(
                    "Trace2 config check for {} {}",
                    target.label,
                    result.status.as_str()
                ));
                result
            })
            .collect()
    };
    let attribution_handles: Vec<_> = targets
        .clone()
        .into_iter()
        .map(|target| {
            let label = target.label.clone();
            debug_progress(format!("starting attribution self-check for {}", label));
            std::thread::spawn(move || {
                let result = run_attribution_self_check(&target);
                debug_progress(format!(
                    "attribution self-check for {} {}",
                    label,
                    result.status.as_str()
                ));
                result
            })
        })
        .collect();
    let attributions: Vec<_> = attribution_handles
        .into_iter()
        .map(|handle| {
            handle.join().unwrap_or_else(|_| {
                DiagnosticCheckResult::failed(
                    "attribution self-check failed",
                    vec!["attribution self-check worker panicked".to_string()],
                    Vec::new(),
                )
            })
        })
        .collect();
    // Trace2 file checks temporarily rewrite global git config, so they must remain serialized.
    let trace2_checks: Vec<_> = if options.skip_trace2_checks {
        debug_progress(format!(
            "skipping Trace2 file self-checks ({})",
            SKIP_TRACE2_CHECKS_FLAG
        ));
        targets
            .iter()
            .map(|_| skipped_trace2_check("trace2 file self-check skipped"))
            .collect()
    } else {
        targets
            .iter()
            .map(|target| {
                debug_progress(format!(
                    "starting Trace2 file self-check for {}",
                    target.label
                ));
                let result = run_trace2_file_self_check(target);
                debug_progress(format!(
                    "Trace2 file self-check for {} {}",
                    target.label,
                    result.status.as_str()
                ));
                result
            })
            .collect()
    };

    targets
        .into_iter()
        .zip(trace2_configs)
        .zip(attributions)
        .zip(trace2_checks)
        .map(
            |(((target, trace2_config), attribution), trace2)| GitDebugDiagnostics {
                target,
                trace2_config,
                attribution,
                trace2,
            },
        )
        .collect()
}

pub(super) fn skipped_trace2_check(summary: &str) -> DiagnosticCheckResult {
    DiagnosticCheckResult::skipped(
        summary,
        vec![format!("skipped by {}", SKIP_TRACE2_CHECKS_FLAG)],
    )
}

pub(super) fn append_git_diagnostics(
    out: &mut String,
    daemon: &DiagnosticCheckResult,
    diagnostics: &[GitDebugDiagnostics],
) {
    let _ = writeln!(out, "== Git Self Checks ==");
    let _ = writeln!(out, "daemon");
    append_diagnostic_check(out, "Daemon check", daemon, false);
    for diagnostic in diagnostics {
        let _ = writeln!(
            out,
            "{} (program: {})",
            diagnostic.target.label, diagnostic.target.program
        );
        append_diagnostic_check(out, "Trace2 config check", &diagnostic.trace2_config, false);
        append_diagnostic_check(
            out,
            "Attribution self-check",
            &diagnostic.attribution,
            false,
        );
        append_diagnostic_check(out, "Trace2 file self-check", &diagnostic.trace2, true);
    }
}

pub(super) fn append_diagnostic_check(
    out: &mut String,
    label: &str,
    check: &DiagnosticCheckResult,
    always_show_trace2: bool,
) {
    let _ = writeln!(
        out,
        "  {}: {} - {}",
        label,
        check.status.as_str(),
        check.summary
    );
    for detail in &check.details {
        let _ = writeln!(out, "    {}", detail);
    }

    if always_show_trace2 && let Some(trace2_json) = check.trace2_json.as_ref() {
        let _ = writeln!(out, "    trace2 JSON received:");
        append_indented_block_with_prefix(out, trace2_json, "      ");
    }

    if check.status == DiagnosticStatus::Failed {
        let _ = writeln!(out, "    command log:");
        for command in &check.commands {
            let _ = writeln!(out, "      $ {}", command.command);
            if let Some(cwd) = &command.cwd {
                let _ = writeln!(out, "        cwd: {}", cwd);
            }
            let _ = writeln!(
                out,
                "        status: {}",
                if command.timed_out {
                    "<timeout>".to_string()
                } else {
                    command
                        .status
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "<unavailable>".to_string())
                }
            );
            if command.timed_out {
                let _ = writeln!(out, "        timed out: yes");
            }
            if !command.stdout.trim().is_empty() {
                let _ = writeln!(out, "        stdout:");
                append_indented_block_with_prefix(out, &command.stdout, "          ");
            }
            if !command.stderr.trim().is_empty() {
                let _ = writeln!(out, "        stderr:");
                append_indented_block_with_prefix(out, &command.stderr, "          ");
            }
        }
    }
}

impl std::fmt::Display for GitVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

pub(super) fn append_git_version_check(out: &mut String, label: &str, version_output: &str) {
    match parse_git_version(version_output) {
        Some(version) if version >= MIN_GIT_VERSION => {
            let _ = writeln!(
                out,
                "{}: version meets or exceeds minimum version of {}",
                label, MIN_GIT_VERSION_DISPLAY
            );
        }
        Some(version) => {
            let _ = writeln!(
                out,
                "{}: ERROR: detected Git version {} is below minimum version {}",
                label, version, MIN_GIT_VERSION_DISPLAY
            );
        }
        None => {
            let _ = writeln!(
                out,
                "{}: <error: could not parse Git version from '{}'; minimum version is {}>",
                label, version_output, MIN_GIT_VERSION_DISPLAY
            );
        }
    }
}

pub(super) fn parse_git_version(output: &str) -> Option<GitVersion> {
    output.split_whitespace().find_map(parse_git_version_token)
}

pub(super) fn parse_git_version_token(token: &str) -> Option<GitVersion> {
    let token = token.trim_start_matches('v');
    let mut parts = token.split('.');
    let major = parse_leading_u32(parts.next()?)?;
    let minor = parse_leading_u32(parts.next()?)?;
    let patch = parts.next().map(parse_leading_u32).unwrap_or(Some(0))?;

    Some(GitVersion {
        major,
        minor,
        patch,
    })
}

pub(super) fn parse_leading_u32(value: &str) -> Option<u32> {
    let digits = value
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

pub(super) const MIN_GIT_VERSION: GitVersion = GitVersion {
    major: 2,
    minor: 22,
    patch: 0,
};
pub(super) const MIN_GIT_VERSION_DISPLAY: &str = "2.22.0";

pub(super) fn collect_platform_info() -> PlatformInfo {
    PlatformInfo {
        kernel: collect_kernel_details(),
        hostname: collect_hostname(),
    }
}

pub(super) fn collect_kernel_details() -> Option<String> {
    #[cfg(unix)]
    {
        run_command_capture("uname", &["-srm"]).ok()
    }
    #[cfg(windows)]
    {
        run_command_capture("cmd", &["/C", "ver"]).ok()
    }
}

pub(super) fn collect_hostname() -> Option<String> {
    if let Ok(hostname) = env::var("HOSTNAME")
        && !hostname.trim().is_empty()
    {
        return Some(hostname);
    }

    if let Ok(hostname) = env::var("COMPUTERNAME")
        && !hostname.trim().is_empty()
    {
        return Some(hostname);
    }

    run_command_capture("hostname", &[]).ok()
}

pub(super) fn collect_hardware_info() -> HardwareInfo {
    let mut info = HardwareInfo {
        logical_cores: std::thread::available_parallelism()
            .ok()
            .map(std::num::NonZeroUsize::get),
        ..HardwareInfo::default()
    };

    #[cfg(target_os = "macos")]
    {
        info.cpu_model = run_command_capture("sysctl", &["-n", "machdep.cpu.brand_string"]).ok();
        info.physical_cores = run_command_capture("sysctl", &["-n", "hw.physicalcpu"])
            .ok()
            .and_then(|s| s.parse::<usize>().ok());
        info.logical_cores = run_command_capture("sysctl", &["-n", "hw.logicalcpu"])
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .or(info.logical_cores);
        info.total_memory_bytes = run_command_capture("sysctl", &["-n", "hw.memsize"])
            .ok()
            .and_then(|s| s.parse::<u64>().ok());
    }

    #[cfg(target_os = "linux")]
    {
        info.cpu_model = read_linux_cpu_model();
        info.total_memory_bytes = read_linux_total_memory();
    }

    #[cfg(windows)]
    {
        info.cpu_model = run_command_capture(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name)",
            ],
        )
        .ok()
        .filter(|s| !s.trim().is_empty());

        info.physical_cores = run_command_capture(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty NumberOfCores)",
            ],
        )
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok());

        info.total_memory_bytes = run_command_capture(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem | Select-Object -ExpandProperty TotalPhysicalMemory)",
            ],
        )
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());
    }

    info
}

#[cfg(target_os = "linux")]
pub(super) fn read_linux_cpu_model() -> Option<String> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").ok()?;
    for line in cpuinfo.lines() {
        if let Some((_, value)) = line.split_once(':')
            && line.starts_with("model name")
        {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub(super) fn read_linux_total_memory() -> Option<u64> {
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
}

pub(super) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.2} {} ({} bytes)", value, UNITS[unit], bytes)
}

pub(super) fn collect_repository_info() -> RepositoryInfo {
    let cwd = env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());

    let repo = match find_repository_in_path(&cwd) {
        Ok(repo) => repo,
        Err(e) => {
            return RepositoryInfo {
                in_repository: false,
                error: Some(e.to_string()),
                workdir: None,
                git_dir: None,
                common_dir: None,
                branch: None,
                head: None,
                hooks_path: None,
                remotes: Vec::new(),
                committer_identity: None,
            };
        }
    };

    let head = repo.head().ok();
    let committer_identity = repo.git_author_identity_resolution();

    RepositoryInfo {
        in_repository: true,
        error: None,
        workdir: repo.workdir().ok().map(|p| p.display().to_string()),
        git_dir: Some(repo.path().display().to_string()),
        common_dir: Some(repo.common_dir().display().to_string()),
        branch: head.as_ref().and_then(|h| h.shorthand().ok()),
        head: head.as_ref().and_then(|h| h.target().ok()),
        hooks_path: repo.config_get_str("core.hooksPath").ok().flatten(),
        remotes: repo.remotes_with_urls().unwrap_or_default(),
        committer_identity: Some(committer_identity),
    }
}

pub(super) fn collect_git_config_dump(git_cmd: &str) -> GitConfigDump {
    let attempts: &[&[&str]] = &[
        &["config", "--list", "--show-origin", "--show-scope"],
        &["config", "--list", "--show-origin"],
        &["config", "--list"],
    ];

    let mut last_error = String::new();
    for args in attempts {
        match run_git_command_capture(git_cmd, args) {
            Ok(output) => {
                let redacted = output
                    .lines()
                    .map(redact_git_config_line)
                    .collect::<Vec<_>>()
                    .join("\n");
                return GitConfigDump {
                    command: format!("{} {}", git_cmd, args.join(" ")),
                    output: Ok(redacted),
                };
            }
            Err(err) => {
                last_error = err;
            }
        }
    }

    GitConfigDump {
        command: format!("{} config --list --show-origin --show-scope", git_cmd),
        output: Err(last_error),
    }
}

pub(super) fn redact_git_config_line(line: &str) -> String {
    if !line.contains('\t') {
        if let Some((key, value)) = line.split_once('=')
            && should_redact_key_value(key, value)
        {
            return format!("{}=[REDACTED]", key);
        }
        return line.to_string();
    }

    let mut parts = line.splitn(3, '\t');
    let first = match parts.next() {
        Some(v) => v,
        None => return line.to_string(),
    };
    let second = match parts.next() {
        Some(v) => v,
        None => return line.to_string(),
    };

    match parts.next() {
        // 3-field format: scope \t origin \t key=value
        // (from `git config --list --show-origin --show-scope`)
        Some(key_value) => {
            let (key, value) = match key_value.split_once('=') {
                Some((key, value)) => (key, value),
                None => return line.to_string(),
            };
            if should_redact_key_value(key, value) {
                format!("{}\t{}\t{}=[REDACTED]", first, second, key)
            } else {
                line.to_string()
            }
        }
        // 2-field format: origin \t key=value
        // (from `git config --list --show-origin` without --show-scope)
        None => {
            let (key, value) = match second.split_once('=') {
                Some((key, value)) => (key, value),
                None => return line.to_string(),
            };
            if should_redact_key_value(key, value) {
                format!("{}\t{}=[REDACTED]", first, key)
            } else {
                line.to_string()
            }
        }
    }
}

pub(super) fn should_redact_key_value(key: &str, value: &str) -> bool {
    let key_lower = key.to_lowercase();
    let value_lower = value.to_lowercase();

    let sensitive_key_markers = [
        "password",
        "passwd",
        "token",
        "secret",
        "oauth",
        "authorization",
        "apikey",
        "api_key",
        "extraheader",
    ];

    if sensitive_key_markers
        .iter()
        .any(|marker| key_lower.contains(marker))
    {
        return true;
    }

    if key_lower.starts_with("url.") {
        return true;
    }

    sensitive_key_markers
        .iter()
        .any(|marker| value_lower.contains(marker))
}

pub(super) fn collect_git_ai_config_dump() -> Result<String, String> {
    let runtime = config::Config::get();
    let mut out = String::new();
    let config_path = config::config_file_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unavailable>".to_string());
    let git_ai_dir = config::git_ai_dir_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unavailable>".to_string());

    let _ = writeln!(out, "config_file_path: {}", config_path);
    let _ = writeln!(out, "git_ai_dir: {}", git_ai_dir);
    let _ = writeln!(out, "runtime_config:");
    let serialized = runtime.to_printable_json_pretty()?;
    append_indented_block(&mut out, &serialized);
    Ok(out)
}

pub(super) fn collect_git_environment() -> Vec<String> {
    collect_git_environment_entries(env::vars())
}

pub(super) fn collect_git_environment_entries<I>(entries: I) -> Vec<String>
where
    I: IntoIterator<Item = (String, String)>,
{
    let mut entries: Vec<(String, String)> = entries
        .into_iter()
        .filter(|(key, _)| is_debug_git_env_key(key))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    entries
        .into_iter()
        .map(|(key, value)| format!("{}={}", key, redact_env_value(&key, &value)))
        .collect()
}

pub(super) fn is_debug_git_env_key(key: &str) -> bool {
    key.starts_with("GIT_AI_") || key.starts_with("GITAI_") || key.starts_with("GIT_")
}

pub(super) fn redact_env_value(key: &str, value: &str) -> String {
    let key_lower = key.to_lowercase();
    let sensitive_markers = ["token", "secret", "password", "key"];
    if sensitive_markers
        .iter()
        .any(|marker| key_lower.contains(marker))
    {
        return "[REDACTED]".to_string();
    }

    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "<empty>".to_string();
    }

    if trimmed.len() > 200 {
        let truncated: String = trimmed.chars().take(200).collect();
        return format!("{}...[truncated]", truncated);
    }

    trimmed.to_string()
}

pub(super) fn run_command_capture(program: &str, args: &[&str]) -> Result<String, String> {
    run_command_capture_with_timeout(program, args, DEBUG_COMMAND_TIMEOUT)
}

pub(super) fn run_git_command_capture(program: &str, args: &[&str]) -> Result<String, String> {
    run_git_command_capture_with_timeout(program, args, DEBUG_COMMAND_TIMEOUT)
}

pub(super) fn run_command_capture_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    run_command_capture_with_timeout_and_env(program, args, timeout, &[], &[])
}

pub(super) fn run_git_command_capture_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    run_command_capture_with_timeout_and_env(
        program,
        args,
        timeout,
        crate::clients::git_cli::INTERNAL_GIT_ENV_REMOVE,
        crate::clients::git_cli::INTERNAL_GIT_ENV_SET,
    )
}

pub(super) fn run_command_capture_with_timeout_and_env(
    program: &str,
    args: &[&str],
    timeout: Duration,
    env_remove: &[&str],
    env_set: &[(&str, &str)],
) -> Result<String, String> {
    let command = format_command_for_error(program, args);
    let output = run_command_with_timeout_and_env(
        program,
        args,
        None,
        timeout,
        DEBUG_COMMAND_POLL_INTERVAL,
        env_remove,
        env_set,
    )
    .map_err(|e| {
        format!(
            "failed to execute '{}': {}",
            program,
            strip_execute_prefix(&e)
        )
    })?;

    capture_result(&command, timeout, output)
}

pub(super) fn capture_result(
    command: &str,
    timeout: Duration,
    output: TimedCommandOutput,
) -> Result<String, String> {
    if output.timed_out {
        return Err(format_timeout_capture_error(command, timeout, output));
    }
    if output.wait_error.is_some() {
        return Err(format_wait_capture_error(command, output));
    }

    command_output_to_result(output)
}

pub(super) fn command_output_to_result(output: TimedCommandOutput) -> Result<String, String> {
    if output.status != Some(0) {
        let mut stderr = output.stderr.trim().to_string();
        append_debug_diagnostics(&mut stderr, &output.diagnostics);
        let code = output
            .status
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        if stderr.is_empty() {
            return Err(format!("exit code {}", code));
        }
        return Err(format!("exit code {}: {}", code, stderr));
    }

    Ok(output.stdout)
}

pub(super) fn format_timeout_capture_error(
    command: &str,
    timeout: Duration,
    output: TimedCommandOutput,
) -> String {
    let mut message = format!(
        "timed out after {:.1}s running '{}'",
        timeout.as_secs_f64(),
        command
    );
    append_debug_diagnostics(&mut message, &output.diagnostics);
    if let Some(wait_error) = output.wait_error {
        message.push_str(&format!("; failed while waiting: {}", wait_error));
    }
    if !output.stdout.trim().is_empty() {
        message.push_str(&format!(
            "; stdout before timeout: {}",
            output.stdout.trim()
        ));
    }
    if !output.stderr.trim().is_empty() {
        message.push_str(&format!(
            "; stderr before timeout: {}",
            output.stderr.trim()
        ));
    }
    message
}

pub(super) fn format_wait_capture_error(command: &str, output: TimedCommandOutput) -> String {
    let wait_error = output.wait_error.as_deref().unwrap_or("unknown wait error");
    let mut message = format!("failed while waiting for '{}': {}", command, wait_error);
    append_debug_diagnostics(&mut message, &output.diagnostics);
    if !output.stdout.trim().is_empty() {
        message.push_str(&format!(
            "; stdout before wait failure: {}",
            output.stdout.trim()
        ));
    }
    if !output.stderr.trim().is_empty() {
        message.push_str(&format!(
            "; stderr before wait failure: {}",
            output.stderr.trim()
        ));
    }
    message
}

pub(super) fn append_debug_diagnostics(message: &mut String, diagnostics: &[String]) {
    for diagnostic in diagnostics {
        if !message.is_empty() {
            message.push_str("; ");
        }
        message.push_str(diagnostic);
    }
}

pub(super) fn strip_execute_prefix(error: &str) -> &str {
    error.strip_prefix("failed to execute: ").unwrap_or(error)
}

const DEBUG_COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const DEBUG_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(super) fn debug_progress(message: impl AsRef<str>) {
    eprintln!(
        "[{}] git-ai debug: {}",
        chrono::Utc::now().to_rfc3339(),
        message.as_ref()
    );
}

pub(super) fn append_indented_block(out: &mut String, content: &str) {
    append_indented_block_with_prefix(out, content, "  ");
}

pub(super) fn append_indented_block_with_prefix(out: &mut String, content: &str, prefix: &str) {
    if content.trim().is_empty() {
        let _ = writeln!(out, "{}<empty>", prefix);
        return;
    }
    for line in content.lines() {
        let _ = writeln!(out, "{}{}", prefix, line);
    }
}
