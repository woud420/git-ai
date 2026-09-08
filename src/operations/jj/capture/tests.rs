use super::{
    CaptureBudget, CaptureHooks, CaptureLimits, CapturePhase, CapturedJjCurrentState,
    JjCaptureError, capture_with,
};
use crate::model::jj_observation::JjOperationEvidence;
use crate::operations::workspace_context::{WorkspaceContext, discover};
use crate::regular_file::MetadataReadBudget;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[allow(dead_code)]
#[path = "../../../../tests/integration/jj_evidence_vectors.rs"]
mod evidence_vectors;
#[allow(dead_code)]
#[path = "../../../../tests/integration/jj_view_vectors.rs"]
mod view_vectors;

use evidence_vectors::{LEFT_HEX, LEFT_ID, MERGE_HEX, MERGE_ID, RIGHT_HEX, RIGHT_ID};
use view_vectors::{MINIMAL_HEX, MINIMAL_ID, RICH_HEX, RICH_ID};

mod budgets;
mod failed_open;
mod mutations;
mod support;
pub(super) use support::Fixture;
use support::{Hooks, attempt, deadline, no_hooks, require_error};
