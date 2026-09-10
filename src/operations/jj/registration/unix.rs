use super::{JjRegistrationError as E, JjRegistrationOutcome, RegisteredJjCurrentState};
use crate::config::Config;
use crate::model::repository::jj_observation_journal::registration::{
    PreparedRegistrationInstall, source_root_guard, workspace_locator_guard,
};
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::jj::baseline::prepare_current_state_baseline;
use crate::operations::jj::capture::registration::{RegistrationCaptureBudget, RetainedCapture};
use crate::operations::workspace_context::WorkspaceContext;
use rand::{TryRng, rngs::SysRng};
use std::time::Instant;

#[path = "history.rs"]
pub(super) mod history;
#[path = "policy.rs"]
pub(in crate::operations::jj) mod policy;
#[path = "saved.rs"]
pub(in crate::operations::jj) mod saved;

use policy::{authorize, check_deadline, require_opt_in};

pub(super) fn register(
    journal: &mut JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<JjRegistrationOutcome, E> {
    require_opt_in(config)?;
    let mut capture_budget = RegistrationCaptureBudget::new(deadline);
    let mut initial = capture_budget
        .open_initial(context)
        .map_err(|error| E::caused("capture", error))?;
    authorize(&mut initial, config, deadline)?;
    if initial.seal().is_some() {
        return reopen_present(journal, initial, deadline, read_budget)
            .map(JjRegistrationOutcome::AlreadyRegistered);
    }
    require_empty_guards(journal, &initial)?;
    check_deadline(deadline)?;
    let (source_id, attachment_id) = fresh_ids()?;
    let created = initial
        .publish_new(&source_id)
        .map_err(|error| E::caused("seal publication", error))?;
    let mut current = capture_budget
        .open_created(context, created)
        .map_err(|error| E::caused("registration capture", error))?;
    authorize(&mut current, config, deadline)?;
    let metadata = current
        .registration_metadata(&attachment_id)
        .map_err(|error| E::caused("registration metadata", error))?;
    let capture = current.captured();
    let anchors: Vec<_> = capture.anchors().iter().collect();
    let prepared = PreparedRegistrationInstall::new(
        &source_id,
        capture.reader_profile(),
        capture.head_ids(),
        &anchors,
        metadata,
    )
    .map_err(|error| E::caused("registration preparation", error))?;
    check_deadline(deadline)?;
    let staged = journal
        .stage_registration_install(prepared, read_budget)
        .map_err(|error| E::caused("registration staging", error))?;
    saved::validate(&current, staged.snapshot())?;
    let native = &staged.snapshot().native.record;
    prepare_current_state_baseline(
        &native.reader_profile,
        &native.captured_head_ids,
        &native.anchors,
    )
    .map_err(|error| E::caused("saved baseline", error))?;
    check_deadline(deadline)?;
    current
        .final_recheck()
        .map_err(|error| E::caused("registration recheck", error))?;
    let snapshot = staged
        .commit()
        .map_err(|error| E::caused("registration commit", error))?;
    saved::finish(snapshot, &current.into_captured()).map(JjRegistrationOutcome::Installed)
}

pub(super) fn reopen(
    journal: &JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<Option<RegisteredJjCurrentState>, E> {
    require_opt_in(config)?;
    let mut capture_budget = RegistrationCaptureBudget::new(deadline);
    let mut current = capture_budget
        .open_initial(context)
        .map_err(|error| E::caused("capture", error))?;
    authorize(&mut current, config, deadline)?;
    if current.seal().is_none() {
        require_empty_guards(journal, &current)?;
        check_deadline(deadline)?;
        current
            .final_recheck()
            .map_err(|error| E::caused("absence recheck", error))?;
        return Ok(None);
    }
    reopen_present(journal, current, deadline, read_budget).map(Some)
}

fn reopen_present(
    journal: &JjObservationJournal,
    mut current: RetainedCapture<'_>,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<RegisteredJjCurrentState, E> {
    let registered = load_registered_context(journal, &current, deadline, read_budget)?;
    check_deadline(deadline)?;
    current
        .final_recheck()
        .map_err(|error| E::caused("registration recheck", error))?;
    Ok(registered)
}

pub(in crate::operations::jj) fn load_registered_context(
    journal: &JjObservationJournal,
    current: &RetainedCapture<'_>,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<RegisteredJjCurrentState, E> {
    let seal = current
        .seal()
        .ok_or_else(|| E::invalid("seal", "registered source seal is absent"))?;
    check_deadline(deadline)?;
    let snapshot = journal
        .read_registration_snapshot(
            seal.source_id(),
            &current.captured().checkout().workspace_name,
            read_budget,
        )
        .map_err(|error| E::caused("saved registration", error))?
        .ok_or_else(|| E::invalid("saved registration", "seal has no complete registration"))?;
    saved::validate(current, &snapshot)?;
    saved::finish(snapshot, current.captured())
}

fn require_empty_guards(
    journal: &JjObservationJournal,
    current: &RetainedCapture<'_>,
) -> Result<(), E> {
    let source_key = source_root_guard(&current.source_binding())
        .map_err(|error| E::caused("source guard", error))?;
    let locator = current
        .workspace_locator()
        .map_err(|error| E::caused("workspace locator", error))?;
    let locator_key =
        workspace_locator_guard(&locator).map_err(|error| E::caused("workspace guard", error))?;
    if journal
        .registration_guards_occupied(&source_key, &locator_key)
        .map_err(|error| E::caused("registration guards", error))?
    {
        return Err(E::invalid(
            "registration guards",
            "source or workspace has retained registration state",
        ));
    }
    Ok(())
}

fn fresh_ids() -> Result<(String, String), E> {
    let mut bytes = [0u8; 64];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|error| E::caused("identifier entropy", error))?;
    if bytes[..32] == bytes[32..] {
        return Err(E::invalid(
            "identifier entropy",
            "source and attachment identifiers collided",
        ));
    }
    Ok((
        crate::operations::jj::content_hash::hex(&bytes[..32]),
        crate::operations::jj::content_hash::hex(&bytes[32..]),
    ))
}
