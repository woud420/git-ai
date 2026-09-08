#[cfg(test)]
use super::budget::CaptureCounters;
use super::budget::{CaptureBudget, CaptureLimits, DirectCapture};
use super::directories::{DirectoryRegistry, EntryKind, kind_at, validate_locator};
use super::metadata::MetadataSamples;
use super::{
    CapturedJjCurrentState, CapturedJjSourceBinding, DirectoryIdentity, JjCaptureError as E,
};
use crate::operations::workspace_context::WorkspaceContext;
use std::path::PathBuf;
use std::time::Instant;

mod facts;
mod history;
use history::{HistoryHooks, HistoryState};
mod retained;
mod seal;
pub(crate) use seal::SourceSeal;
use seal::{HeldSeal, SealHooks, SealSample};

#[derive(PartialEq, Eq)]
pub(crate) struct SampledPolicyPaths {
    pub(crate) workspace_root: PathBuf,
    pub(crate) git_dir: PathBuf,
    pub(crate) git_common_dir: PathBuf,
}

pub(crate) struct RegistrationCaptureBudget {
    sessions: [CaptureBudget; 2],
    opened: [bool; 2],
}

impl RegistrationCaptureBudget {
    pub(crate) fn new(deadline: Instant) -> Self {
        Self::with_limits(
            deadline,
            [CaptureLimits::default(), CaptureLimits::default()],
        )
    }

    fn with_limits(deadline: Instant, limits: [CaptureLimits; 2]) -> Self {
        Self {
            sessions: limits.map(|mut limits| {
                limits.live_directory_descriptors = limits.live_directory_descriptors.min(254);
                CaptureBudget::with_limits(deadline, limits)
            }),
            opened: [false; 2],
        }
    }

    pub(crate) fn open_initial<'a>(
        &'a mut self,
        context: &WorkspaceContext,
    ) -> Result<RetainedCapture<'a>, E> {
        self.open_initial_with(context, &mut DirectCapture)
    }

    pub(crate) fn open_created<'a>(
        &'a mut self,
        context: &WorkspaceContext,
        created: CreatedSourceSeal,
    ) -> Result<RetainedCapture<'a>, E> {
        self.open_created_with(context, created, &mut DirectCapture)
    }

    fn open_initial_with<'a>(
        &'a mut self,
        context: &WorkspaceContext,
        hooks: &mut impl SealHooks,
    ) -> Result<RetainedCapture<'a>, E> {
        self.once(0)?;
        RetainedCapture::open(context, &mut self.sessions[0], None, false, hooks)
    }

    fn open_created_with<'a>(
        &'a mut self,
        context: &WorkspaceContext,
        created: CreatedSourceSeal,
        hooks: &mut impl SealHooks,
    ) -> Result<RetainedCapture<'a>, E> {
        if !self.opened[0] {
            return Err(E::invalid(
                "registration capture",
                "initial session was not opened",
            ));
        }
        self.once(1)?;
        RetainedCapture::open(context, &mut self.sessions[1], Some(created), false, hooks)
    }

    fn once(&mut self, index: usize) -> Result<(), E> {
        if std::mem::replace(&mut self.opened[index], true) {
            return Err(E::invalid("registration capture", "session already opened"));
        }
        Ok(())
    }

    #[cfg(test)]
    fn session_counters(&self, index: usize) -> CaptureCounters {
        self.sessions[index].counters()
    }
}

pub(crate) struct HistoryCaptureBudget {
    budget: CaptureBudget,
    opened: bool,
}

impl HistoryCaptureBudget {
    pub(crate) fn new(deadline: Instant) -> Self {
        Self::with_limits(deadline, CaptureLimits::default())
    }

    fn with_limits(deadline: Instant, mut limits: CaptureLimits) -> Self {
        limits.live_directory_descriptors = limits.live_directory_descriptors.min(254);
        let mut budget = CaptureBudget::with_limits(deadline, limits);
        budget.metadata =
            crate::regular_file::MetadataReadBudget::new(10 * 1024 * 1024, 640, deadline);
        Self {
            budget,
            opened: false,
        }
    }

    pub(crate) fn open<'a>(
        &'a mut self,
        context: &WorkspaceContext,
    ) -> Result<RetainedCapture<'a>, E> {
        self.open_with(context, &mut DirectCapture)
    }

    fn open_with<'a>(
        &'a mut self,
        context: &WorkspaceContext,
        hooks: &mut impl HistoryHooks,
    ) -> Result<RetainedCapture<'a>, E> {
        if std::mem::replace(&mut self.opened, true) {
            return Err(E::invalid("history", "session already opened"));
        }
        RetainedCapture::open(context, &mut self.budget, None, true, hooks)
    }
}

pub(crate) struct CreatedSourceSeal {
    held: HeldSeal,
    continuity: Continuity,
}

struct Continuity {
    source: CapturedJjSourceBinding,
    workspace: [DirectoryIdentity; 4],
    colocated: bool,
    policy: SampledPolicyPaths,
}

pub(crate) struct RetainedCapture<'a> {
    directories: DirectoryRegistry,
    metadata: MetadataSamples,
    budget: &'a mut CaptureBudget,
    captured: Option<CapturedJjCurrentState>,
    repository: usize,
    policy_directories: [usize; 3],
    operation_directories: [usize; 2],
    policy_paths: SampledPolicyPaths,
    canonical_paths: Option<SampledPolicyPaths>,
    colocated: bool,
    namespace: Option<usize>,
    held: Option<HeldSeal>,
    expected_policy: Option<SampledPolicyPaths>,
    final_checked: bool,
    final_succeeded: bool,
    history_mode: bool,
    history_state: HistoryState,
}

impl RetainedCapture<'_> {
    pub(crate) fn captured(&self) -> &CapturedJjCurrentState {
        // No session is exposed until sampling succeeds; publication consumes
        // the session before removing this value and never returns it again.
        self.captured
            .as_ref()
            .expect("retained capture has sampled evidence")
    }

    pub(crate) fn seal(&self) -> Option<&SourceSeal> {
        self.held.as_ref().map(|held| &held.seal)
    }
    pub(crate) fn policy_paths(&self) -> &SampledPolicyPaths {
        &self.policy_paths
    }

    pub(crate) fn validate_policy_paths(&mut self, canonical: SampledPolicyPaths) -> Result<(), E> {
        if self.canonical_paths.is_some() || self.final_checked {
            return Err(E::invalid(
                "policy binding",
                "policy paths already validated or session finished",
            ));
        }
        let paths = [
            &canonical.workspace_root,
            &canonical.git_dir,
            &canonical.git_common_dir,
        ];
        for (path, expected) in paths.into_iter().zip(self.policy_directories) {
            validate_locator(path)?;
            use std::os::unix::ffi::OsStrExt;
            let actual = self.directories.walk(
                0,
                path.as_os_str().as_bytes(),
                self.budget,
                &mut DirectCapture,
            )?;
            if self.directories.identity(actual) != self.directories.identity(expected) {
                return Err(E::invalid(
                    "policy binding",
                    "canonical policy directory differs from sampled identity",
                ));
            }
        }
        if self
            .expected_policy
            .as_ref()
            .is_some_and(|expected| expected != &canonical)
        {
            return Err(E::invalid(
                "policy binding",
                "canonical policy paths changed between registration sessions",
            ));
        }
        self.canonical_paths = Some(canonical);
        Ok(())
    }

    pub(crate) fn final_recheck(&mut self) -> Result<(), E> {
        self.final_recheck_with(&mut DirectCapture)
    }

    fn final_recheck_with(&mut self, hooks: &mut impl SealHooks) -> Result<(), E> {
        if std::mem::replace(&mut self.final_checked, true) {
            return Err(E::invalid(
                "registration capture",
                "final recheck already attempted",
            ));
        }
        self.metadata
            .recheck(&self.directories, self.budget, hooks)?;
        match (&self.held, self.namespace) {
            (Some(held), Some(index)) => held.recheck(
                self.directories.file(index),
                SealSample::Final,
                self.budget,
                hooks,
            )?,
            (None, None) => {
                if kind_at(
                    self.directories.file(self.repository),
                    c"git-ai",
                    self.budget,
                    hooks,
                )?
                .is_some()
                {
                    return Err(E::invalid("seal", "namespace appeared after absent sample"));
                }
            }
            _ => {
                return Err(E::invalid(
                    "registration capture",
                    "inconsistent retained namespace",
                ));
            }
        }
        self.directories.recheck(self.budget, hooks)?;
        self.final_succeeded = true;
        Ok(())
    }

    pub(crate) fn into_captured(mut self) -> CapturedJjCurrentState {
        let mut captured = self
            .captured
            .take()
            .expect("retained capture has sampled evidence");
        captured._metadata = std::mem::take(&mut self.metadata).into_captured(&self.directories);
        captured
    }

    #[cfg(test)]
    fn read_remaining(&self) -> (usize, usize) {
        (
            self.budget.metadata.remaining_bytes(),
            self.budget.metadata.remaining_file_attempts(),
        )
    }
}

impl Drop for RetainedCapture<'_> {
    fn drop(&mut self) {
        self.directories.close(self.budget);
    }
}

#[cfg(test)]
mod tests;
