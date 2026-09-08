use super::*;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct SavedTarget {
    pub(super) workspace_name: String,
    pub(super) attachment_id: String,
}

impl SavedTarget {
    pub(super) fn from_registered(value: &RegisteredJjCurrentState) -> Self {
        Self {
            workspace_name: value.workspace_name().to_owned(),
            attachment_id: value.attachment_id().to_owned(),
        }
    }

    pub(super) fn with<'a>(
        &'a self,
        admission: NativeAdmissionExpectation<'a>,
    ) -> NativeReconciliationExpectation<'a> {
        NativeReconciliationExpectation {
            admission,
            workspace_name: &self.workspace_name,
            attachment_id: &self.attachment_id,
        }
    }
}

pub(super) fn reconcile(
    case: &Case,
    journal: &mut JjObservationJournal,
    config: &Config,
    expected: NativeReconciliationExpectation<'_>,
    until: Instant,
    reads: &mut ReadBudget,
) -> Result<NativeReconciliationOutcome, JjNativeAdmissionError> {
    let before = total_state(case);
    let old = state(case);
    let result =
        reconcile_registered_history(journal, &case.context(), config, expected, until, reads);
    assert!(
        state(case) == old,
        "reconciliation changed source, config, baseline or opaque state"
    );
    if !matches!(
        &result,
        Ok(NativeReconciliationOutcome::Admission(
            NativeAdmissionOutcome::Admitted(_)
        ))
    ) {
        assert!(
            total_state(case) == before,
            "nonwriting reconciliation changed state"
        );
    }
    result
}

pub(super) fn now(
    case: &Case,
    journal: &mut JjObservationJournal,
    config: &Config,
    target: &SavedTarget,
    expected: &NativeAdmissionCursor,
) -> NativeReconciliationOutcome {
    reconcile(
        case,
        journal,
        config,
        target.with(expected.expectation()),
        deadline(),
        &mut admission_budget(),
    )
    .unwrap()
}

pub(super) fn unchanged(value: NativeReconciliationOutcome) -> RegisteredNativeAdmissionState {
    match value {
        NativeReconciliationOutcome::Unchanged(value) => value,
        NativeReconciliationOutcome::Admission(_) => {
            panic!("unchanged heads produced an admission")
        }
    }
}

pub(super) fn admitted(
    value: NativeReconciliationOutcome,
    already: bool,
) -> RegisteredNativeAdmission {
    match value {
        NativeReconciliationOutcome::Admission(value) => outcome(value, already),
        NativeReconciliationOutcome::Unchanged(_) => {
            panic!("changed-head request was silently skipped")
        }
    }
}

pub(super) fn refuse(
    case: &Case,
    journal: &mut JjObservationJournal,
    config: &Config,
    target: &SavedTarget,
    expected: &NativeAdmissionCursor,
) {
    failed(reconcile(
        case,
        journal,
        config,
        target.with(expected.expectation()),
        deadline(),
        &mut admission_budget(),
    ));
}
