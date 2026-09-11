use crate::config;
use crate::operations::daemon::self_check::{DiagnosticCheckResult, GitDiagnosticTarget};
use crate::operations::git::repository::{GitConfigIdentityResolution, GitIdentityResolution};

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DebugOptions {
    pub(super) skip_trace2_checks: bool,
}

pub(super) struct ShellGitLookup {
    pub(super) command: String,
    pub(super) path: Result<String, String>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct GitVersion {
    pub(super) major: u32,
    pub(super) minor: u32,
    pub(super) patch: u32,
}

#[derive(Default)]
pub(super) struct PlatformInfo {
    pub(super) kernel: Option<String>,
    pub(super) hostname: Option<String>,
}

#[derive(Default)]
pub(super) struct HardwareInfo {
    pub(super) cpu_model: Option<String>,
    pub(super) physical_cores: Option<usize>,
    pub(super) logical_cores: Option<usize>,
    pub(super) total_memory_bytes: Option<u64>,
}

pub(super) struct RepositoryInfo {
    pub(super) in_repository: bool,
    pub(super) error: Option<String>,
    pub(super) workdir: Option<String>,
    pub(super) git_dir: Option<String>,
    pub(super) common_dir: Option<String>,
    pub(super) branch: Option<String>,
    pub(super) head: Option<String>,
    pub(super) hooks_path: Option<String>,
    pub(super) remotes: Vec<(String, String)>,
    pub(super) committer_identity: Option<GitIdentityResolution>,
}

pub(super) struct GitConfigDump {
    pub(super) command: String,
    pub(super) output: Result<String, String>,
}

pub(super) const SKIP_TRACE2_CHECKS_FLAG: &str = "--skip-trace2-checks";
