//! Bounded, read-only sampling of one jj source's current heads and checkout.
//!
//! Matching samples are not an atomic filesystem snapshot or lifetime source
//! identity. This off-Trace2 API does not walk ancestry, admit journal records,
//! enable collection, or certify dirty working-file contents.

use super::baseline::{
    JjBaselineError, PreparedCurrentStateBaseline, prepare_current_state_baseline,
};
use super::checkout::DecodedJjCheckout;
use crate::model::jj_observation::{JJ_OBSERVATION_READER_PROFILE, JjOperationEvidence};
use crate::operations::workspace_context::WorkspaceContext;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::time::Instant;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod budget;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod directories;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod evidence;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod heads;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod metadata;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod registration;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod source;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
use budget::CaptureLimits;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use budget::{CaptureBudget, CaptureHooks, CapturePhase, DirectCapture};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use unix::capture_with;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

/// Equality compares sampled backing directories, not permanent source continuity.
#[derive(Debug, PartialEq, Eq)]
pub struct CapturedJjSourceBinding {
    profile: &'static str,
    directories: [DirectoryIdentity; 8],
    backends: [Vec<u8>; 3],
}

struct SampledMetadata {
    _parent: DirectoryIdentity,
    _name: OsString,
    _bytes: Option<Vec<u8>>,
    _directory: Option<DirectoryIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JjCheckoutRelation {
    OperationIsCapturedHead,
    OperationOutsideCapturedHeads,
}

#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
enum CheckoutEvidence {
    Anchor(usize),
    Outside(JjOperationEvidence),
}

/// Owned evidence authenticated at a sampled current-state boundary.
pub struct CapturedJjCurrentState {
    source_binding: CapturedJjSourceBinding,
    head_ids: Vec<String>,
    anchors: Vec<JjOperationEvidence>,
    checkout_bytes: Vec<u8>,
    checkout: DecodedJjCheckout,
    checkout_evidence: CheckoutEvidence,
    _workspace_directories: [DirectoryIdentity; 4],
    _metadata: Vec<SampledMetadata>,
}

impl CapturedJjCurrentState {
    pub fn reader_profile(&self) -> &'static str {
        JJ_OBSERVATION_READER_PROFILE
    }

    pub fn source_binding(&self) -> &CapturedJjSourceBinding {
        &self.source_binding
    }

    pub fn head_ids(&self) -> &[String] {
        &self.head_ids
    }

    pub fn anchors(&self) -> &[JjOperationEvidence] {
        &self.anchors
    }

    pub fn checkout_bytes(&self) -> &[u8] {
        &self.checkout_bytes
    }

    pub fn checkout(&self) -> &DecodedJjCheckout {
        &self.checkout
    }

    pub fn checkout_evidence(&self) -> &JjOperationEvidence {
        match &self.checkout_evidence {
            CheckoutEvidence::Anchor(index) => &self.anchors[*index],
            CheckoutEvidence::Outside(evidence) => evidence,
        }
    }

    pub fn checkout_relation(&self) -> JjCheckoutRelation {
        match self.checkout_evidence {
            CheckoutEvidence::Anchor(_) => JjCheckoutRelation::OperationIsCapturedHead,
            CheckoutEvidence::Outside(_) => JjCheckoutRelation::OperationOutsideCapturedHeads,
        }
    }

    /// Reuses owned bytes without filesystem access; outside checkout is excluded.
    pub fn prepare_baseline(&self) -> Result<PreparedCurrentStateBaseline<'_>, JjBaselineError> {
        prepare_current_state_baseline(self.reader_profile(), &self.head_ids, &self.anchors)
    }
}

#[derive(Debug)]
pub struct JjCaptureError {
    stage: &'static str,
    detail: &'static str,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl JjCaptureError {
    fn invalid(stage: &'static str, detail: &'static str) -> Self {
        Self {
            stage,
            detail,
            source: None,
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn caused(stage: &'static str, source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            stage,
            detail: "",
            source: Some(Box::new(source)),
        }
    }
}

impl fmt::Display for JjCaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "jj capture {}: ", self.stage)?;
        match &self.source {
            Some(source) => fmt::Display::fmt(source, formatter),
            None => formatter.write_str(self.detail),
        }
    }
}

impl Error for JjCaptureError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

/// Revalidates discovery-only locators under fixed limits and a cooperative deadline.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn capture_current_state(
    context: &WorkspaceContext,
    deadline: Instant,
) -> Result<CapturedJjCurrentState, JjCaptureError> {
    capture_with(
        context,
        &mut CaptureBudget::new(deadline),
        &mut DirectCapture,
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn capture_current_state(
    _context: &WorkspaceContext,
    _deadline: Instant,
) -> Result<CapturedJjCurrentState, JjCaptureError> {
    Err(JjCaptureError::invalid(
        "platform",
        "unsupported native capture platform",
    ))
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests;
