use super::formatting::{append_indented_block_with_prefix, debug_progress};
use super::system::RepositoryInfo;
use super::{DebugOptions, SKIP_TRACE2_CHECKS_FLAG};
use crate::config;
use crate::operations::daemon::attribution_self_check::run_attribution_self_check;
use crate::operations::daemon::self_check::{
    DiagnosticCheckResult, DiagnosticStatus, GitDiagnosticTarget,
};
use crate::operations::git::repository::{
    GitAuthorIdentity, GitConfigIdentityResolution, GitIdentityResolution,
    global_git_config_identity_resolution,
};
use crate::operations::git::trace2_validation::{
    check_trace2_global_config, run_trace2_file_self_check,
};
use std::fmt::Write as _;

pub(super) struct GitDebugDiagnostics {
    pub(super) target: GitDiagnosticTarget,
    pub(super) trace2_config: DiagnosticCheckResult,
    pub(super) attribution: DiagnosticCheckResult,
    pub(super) trace2: DiagnosticCheckResult,
}

pub(super) struct GitCommitterIdentityInfo {
    pub(super) global_config: Result<GitConfigIdentityResolution, String>,
    pub(super) repository: RepositoryCommitterIdentity,
    pub(super) author_config: config::AuthorConfig,
}

pub(super) enum RepositoryCommitterIdentity {
    InRepository(GitIdentityResolution),
    NotInRepository(String),
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct GitVersion {
    pub(super) major: u32,
    pub(super) minor: u32,
    pub(super) patch: u32,
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
