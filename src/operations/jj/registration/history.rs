use super::load_registered_context;
use super::policy::{authorize, check_deadline, require_opt_in};
use crate::config::Config;
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::jj::capture::CapturedJjHistoryEvidence;
use crate::operations::jj::capture::registration::HistoryCaptureBudget;
use crate::operations::jj::history::JjHistoryCollectionError;
use crate::operations::jj::registration::RegisteredJjCurrentState;
use crate::operations::workspace_context::WorkspaceContext;
use std::time::Instant;

pub(in crate::operations::jj) fn collect(
    journal: &JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    deadline: Instant,
    read_budget: &mut ReadBudget,
) -> Result<(RegisteredJjCurrentState, CapturedJjHistoryEvidence), JjHistoryCollectionError> {
    require_opt_in(config)?;
    let mut budget = HistoryCaptureBudget::new(deadline);
    let mut current = budget.open(context)?;
    authorize(&mut current, config, deadline)?;
    let registered = load_registered_context(journal, &current, deadline, read_budget)?;
    check_deadline(deadline)?;
    current.collect_history(registered.baseline())?;
    check_deadline(deadline)?;
    current.final_recheck()?;
    let evidence = current.into_history()?;
    Ok((registered, evidence))
}
