use super::{E, NativeAdmissionExpectation, NativeReconciliationExpectation};
use crate::model::jj_observation::validate_source;
use crate::model::repository::jj_observation_journal::registration::validate_workspace_name;

pub(super) struct ExpectedAttempt<'a> {
    pub(super) admission: NativeAdmissionExpectation<'a>,
    target: Option<WorkspaceTarget<'a>>,
}

struct WorkspaceTarget<'a> {
    workspace_name: &'a str,
    attachment_id: &'a str,
}

impl<'a> ExpectedAttempt<'a> {
    pub(super) fn explicit(admission: NativeAdmissionExpectation<'a>) -> Self {
        Self {
            admission,
            target: None,
        }
    }

    pub(super) fn reconciliation(expected: NativeReconciliationExpectation<'a>) -> Self {
        Self {
            admission: expected.admission,
            target: Some(WorkspaceTarget {
                workspace_name: expected.workspace_name,
                attachment_id: expected.attachment_id,
            }),
        }
    }

    pub(super) fn validate_target(&self) -> Result<(), E> {
        if let Some(target) = &self.target {
            validate_workspace_name(target.workspace_name)
                .map_err(|_| E::Input("invalid expected workspace name"))?;
            validate_source(target.attachment_id)
                .map_err(|_| E::Input("invalid expected workspace attachment identity"))?;
        }
        Ok(())
    }

    pub(super) fn require_target(
        &self,
        workspace_name: &str,
        attachment_id: &str,
    ) -> Result<(), E> {
        if let Some(target) = &self.target
            && (target.workspace_name != workspace_name || target.attachment_id != attachment_id)
        {
            return Err(E::Input(
                "expected workspace scope differs from registration",
            ));
        }
        Ok(())
    }
}
