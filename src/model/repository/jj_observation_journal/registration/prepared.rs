use super::super::{codec, native_baseline, registration_records};
use super::*;
use crate::model::jj_observation::JjOperationEvidence;
use registration_records::codec as record_codec;
use registration_records::types::MAX_RECORD_BYTES;

pub(crate) struct PreparedRegistrationInstall<'a> {
    pub(super) native: native_baseline::PreparedBaseline<'a>,
    pub(super) registration: RegistrationRecord,
    pub(super) workspace: WorkspaceRecord,
    pub(super) source_root_key: String,
    pub(super) locator_key: String,
    pub(super) workspace_root_key: String,
    pub(super) registration_bytes: Vec<u8>,
    pub(super) workspace_bytes: Vec<u8>,
    pub(super) registration_checksum: String,
    pub(super) workspace_checksum: String,
}

impl<'a> PreparedRegistrationInstall<'a> {
    pub(crate) fn new(
        source: &'a str,
        profile: &'a str,
        heads: &'a [String],
        anchors: &[&'a JjOperationEvidence],
        metadata: RegistrationMetadata,
    ) -> Result<Self, JournalError> {
        validate_metadata(&metadata)?;
        let request = native_baseline::Request::new(source, 0, profile, heads, anchors)?;
        let native = native_baseline::PreparedBaseline::new(request)?;
        let RegistrationMetadata {
            seal_bytes,
            source_binding,
            workspace,
        } = metadata;
        let seal_digest = codec::checksum(&seal_bytes);
        let workspace = WorkspaceRecord {
            record_version: 1,
            domain: "git-ai/jj/workspace-attachment/v1".to_owned(),
            source_id: source.to_owned(),
            reader_profile: profile.to_owned(),
            baseline_id: native.state.baseline_id.clone(),
            baseline_generation: 1,
            seal_digest: seal_digest.clone(),
            workspace_name: workspace.workspace_name,
            attachment_id: workspace.attachment_id,
            locator: workspace.locator,
            workspace_binding: workspace.workspace_binding,
            selected_checkout: workspace.selected_checkout,
        };
        record_codec::validate_workspace(&workspace)?;
        let workspace_bytes = codec::encode(&workspace, MAX_RECORD_BYTES)?;
        let workspace_checksum = codec::checksum(&workspace_bytes);
        let registration = RegistrationRecord {
            record_version: 1,
            domain: "git-ai/jj/source-registration/v1".to_owned(),
            source_id: source.to_owned(),
            reader_profile: profile.to_owned(),
            baseline_id: native.state.baseline_id.clone(),
            baseline_generation: 1,
            seal_bytes: ByteString(seal_bytes),
            seal_digest,
            source_binding,
            initial_workspace_name: workspace.workspace_name.clone(),
            initial_attachment_id: workspace.attachment_id.clone(),
            initial_workspace_record_id: workspace_checksum.clone(),
        };
        record_codec::validate_registration(&registration)?;
        let registration_bytes = codec::encode(&registration, MAX_RECORD_BYTES)?;
        let registration_checksum = codec::checksum(&registration_bytes);
        Ok(Self {
            source_root_key: source_root_guard(&registration.source_binding)?,
            locator_key: workspace_locator_guard(&workspace.locator)?,
            workspace_root_key: workspace_root_guard(
                &workspace.locator,
                &workspace.workspace_binding,
            )?,
            native,
            registration,
            workspace,
            registration_bytes,
            workspace_bytes,
            registration_checksum,
            workspace_checksum,
        })
    }
}

fn validate_metadata(metadata: &RegistrationMetadata) -> Result<(), JournalError> {
    record_codec::validate_byte_length(&metadata.seal_bytes, 1024)?;
    record_codec::validate_source_binding(&metadata.source_binding)?;
    let workspace = &metadata.workspace;
    record_codec::validate_name(&workspace.workspace_name)?;
    validate_source(&workspace.attachment_id)?;
    record_codec::validate_locator(&workspace.locator)?;
    record_codec::validate_selected_checkout(&workspace.selected_checkout)?;
    if metadata.source_binding.platform != workspace.locator.platform {
        return Err(invalid("native registration workspace platform mismatch"));
    }
    Ok(())
}
