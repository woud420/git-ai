//! Explicit source registration and sampled reopening of a saved native cutoff.
//! A registered result does not enable attribution or prove later ancestry.

use super::baseline_persistence::DurableCurrentStateBaseline;
use super::checkout::DecodedJjCheckout;
use crate::config::Config;
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::workspace_context::WorkspaceContext;
use std::time::Instant;

mod error;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;

pub use error::JjRegistrationError;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use unix as platform;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use unsupported as platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JjRegisteredCheckoutRelation {
    BaselineAnchor,
    OutsideBaseline,
}

/// A saved cutoff joined to a freshly sampled workspace and source seal.
/// This is neither a live descriptor lease nor permission to attribute edits.
pub struct RegisteredJjCurrentState {
    source_id: String,
    initialization_receipt_id: String,
    workspace_name: String,
    attachment_id: String,
    baseline: DurableCurrentStateBaseline,
    checkout: DecodedJjCheckout,
    checkout_relation: JjRegisteredCheckoutRelation,
}

impl RegisteredJjCurrentState {
    pub fn source_id(&self) -> &str {
        &self.source_id
    }
    pub fn initialization_receipt_id(&self) -> &str {
        &self.initialization_receipt_id
    }
    pub fn workspace_name(&self) -> &str {
        &self.workspace_name
    }
    pub fn attachment_id(&self) -> &str {
        &self.attachment_id
    }
    pub fn baseline(&self) -> &DurableCurrentStateBaseline {
        &self.baseline
    }
    pub fn checkout(&self) -> &DecodedJjCheckout {
        &self.checkout
    }
    pub fn checkout_relation(&self) -> JjRegisteredCheckoutRelation {
        self.checkout_relation
    }
}

pub enum JjRegistrationOutcome {
    Installed(RegisteredJjCurrentState),
    AlreadyRegistered(RegisteredJjCurrentState),
}

/// Explicitly initializes one source and its first workspace, or reopens its
/// saved receipt. Existing seals never authorize adoption or a new cutoff.
pub fn register_current_state(
    journal: &mut JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<JjRegistrationOutcome, JjRegistrationError> {
    platform::register(journal, context, config, deadline, read_budget)
}

/// Reopens one complete registration using fresh filesystem and native evidence.
/// Absence is not first-use authority; this function never creates source metadata.
pub fn reopen_registered_current_state(
    journal: &JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<Option<RegisteredJjCurrentState>, JjRegistrationError> {
    platform::reopen(journal, context, config, deadline, read_budget)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use unix::history::collect as collect_history;
