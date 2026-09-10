use super::*;
use crate::model::jj_observation::{JJ_OBSERVATION_READER_PROFILE, JjOperationEvidence};
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::jj::baseline::prepare_current_state_baseline;
use crate::operations::jj::baseline_persistence::{
    DurableCurrentStateBaseline, persist_current_state_baseline, reopen_current_state_baseline,
};
use crate::operations::jj::capture::CapturedJjHistoryEvidence;
use crate::operations::jj::capture::budget::{CaptureHooks, CaptureLimits, CapturePhase};
use crate::operations::jj::capture::registration::HistoryCaptureBudget;
use crate::operations::jj::capture::registration::history::{HistoryHooks, HistoryPhase};
use crate::operations::jj::capture::registration::seal::{SealHooks, SealSample};
use crate::regular_file::MetadataReadBudget;

// Shared independent fixtures also include public-only graph cases.
#[allow(dead_code, unused_imports)]
mod fixtures {
    use crate as git_ai;
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/integration/fixtures/jj-history/helpers.rs"
    ));
}

mod borrowed;
mod bounds;
mod lifecycle;
mod races;
mod support;
use support::*;

mod coordinator;
