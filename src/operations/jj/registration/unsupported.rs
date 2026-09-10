use super::*;

pub(super) fn register(
    _journal: &mut JjObservationJournal,
    _context: &WorkspaceContext,
    _config: &Config,
    _deadline: Instant,
    _read_budget: &mut ReadBudget,
) -> Result<JjRegistrationOutcome, JjRegistrationError> {
    Err(JjRegistrationError::invalid(
        "platform",
        "unsupported native registration platform",
    ))
}

pub(super) fn reopen(
    _journal: &JjObservationJournal,
    _context: &WorkspaceContext,
    _config: &Config,
    _deadline: Instant,
    _read_budget: &mut ReadBudget,
) -> Result<Option<RegisteredJjCurrentState>, JjRegistrationError> {
    Err(JjRegistrationError::invalid(
        "platform",
        "unsupported native registration platform",
    ))
}
