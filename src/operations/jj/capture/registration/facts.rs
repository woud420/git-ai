use super::super::JjCheckoutRelation;
use super::*;
use crate::model::repository::jj_observation_journal::registration::{
    BaselineRelation, ByteString, DirectoryIdentity as ModelIdentity, Platform,
    RegistrationMetadata, SelectedCheckout, SourceBinding, WorkspaceBinding, WorkspaceLocator,
    WorkspaceRegistrationMetadata,
};
use std::os::unix::ffi::OsStrExt;

impl RetainedCapture<'_> {
    pub(crate) fn source_binding(&self) -> SourceBinding {
        let source = self.captured().source_binding();
        SourceBinding {
            platform: platform(),
            identity_format: "unix-device-inode/v1".to_owned(),
            directories: source.directories.map(identity),
            backends: std::array::from_fn(|index| ByteString(source.backends[index].clone())),
        }
    }

    pub(crate) fn workspace_locator(&self) -> Result<WorkspaceLocator, E> {
        let paths = self.canonical_paths.as_ref().ok_or(E::invalid(
            "policy binding",
            "canonical policy paths were not validated",
        ))?;
        Ok(WorkspaceLocator {
            platform: platform(),
            workspace_root: ByteString(paths.workspace_root.as_os_str().as_bytes().to_vec()),
        })
    }

    pub(crate) fn workspace_binding(&self) -> WorkspaceBinding {
        WorkspaceBinding {
            directories: self.captured()._workspace_directories.map(identity),
            colocated: self.colocated,
        }
    }

    pub(crate) fn registration_metadata(
        &self,
        attachment_id: &str,
    ) -> Result<RegistrationMetadata, E> {
        crate::model::jj_observation::validate_source(attachment_id)
            .map_err(|error| E::caused("attachment identity", error))?;
        let seal = self
            .seal()
            .ok_or(E::invalid("seal", "source seal is absent"))?;
        let captured = self.captured();
        let baseline_relation = match captured.checkout_relation() {
            JjCheckoutRelation::OperationIsCapturedHead => BaselineRelation::BaselineAnchor,
            JjCheckoutRelation::OperationOutsideCapturedHeads => BaselineRelation::OutsideBaseline,
        };
        Ok(RegistrationMetadata {
            seal_bytes: seal.bytes().to_vec(),
            source_binding: self.source_binding(),
            workspace: WorkspaceRegistrationMetadata {
                workspace_name: captured.checkout().workspace_name.clone(),
                attachment_id: attachment_id.to_owned(),
                locator: self.workspace_locator()?,
                workspace_binding: self.workspace_binding(),
                selected_checkout: SelectedCheckout {
                    raw_checkout_bytes: ByteString(captured.checkout_bytes().to_vec()),
                    operation_id: captured.checkout().operation_id.clone(),
                    view_id: captured.checkout_evidence().view_id.clone(),
                    baseline_relation,
                },
            },
        })
    }
}

fn identity(value: DirectoryIdentity) -> ModelIdentity {
    ModelIdentity {
        device: value.device,
        inode: value.inode,
    }
}

fn platform() -> Platform {
    #[cfg(target_os = "linux")]
    {
        Platform::Linux
    }
    #[cfg(target_os = "macos")]
    {
        Platform::Macos
    }
}
