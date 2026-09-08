use super::bounded::ByteString;
use serde::{Deserialize, Serialize};

pub(super) const MAX_RECORD_BYTES: usize = 128 * 1024;
pub(super) const MAX_NAME_BYTES: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Platform {
    Linux,
    Macos,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DirectoryIdentity {
    pub(super) device: u64,
    pub(super) inode: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceBinding {
    pub(super) platform: Platform,
    pub(super) identity_format: String,
    pub(super) directories: [DirectoryIdentity; 8],
    pub(super) backends: [ByteString<{ 16 * 1024 }>; 3],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RegistrationRecord {
    pub(super) record_version: u16,
    pub(super) domain: String,
    pub(super) source_id: String,
    pub(super) reader_profile: String,
    pub(super) baseline_id: String,
    pub(super) baseline_generation: u64,
    pub(super) seal_bytes: ByteString<1024>,
    pub(super) seal_digest: String,
    pub(super) source_binding: SourceBinding,
    pub(super) initial_workspace_name: String,
    pub(super) initial_attachment_id: String,
    pub(super) initial_workspace_record_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkspaceLocator {
    pub(super) platform: Platform,
    pub(super) workspace_root: ByteString<{ 64 * 1024 }>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkspaceBinding {
    pub(super) directories: [DirectoryIdentity; 4],
    pub(super) colocated: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum BaselineRelation {
    BaselineAnchor,
    OutsideBaseline,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SelectedCheckout {
    pub(super) raw_checkout_bytes: ByteString<{ 16 * 1024 }>,
    pub(super) operation_id: String,
    pub(super) view_id: String,
    pub(super) baseline_relation: BaselineRelation,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkspaceRecord {
    pub(super) record_version: u16,
    pub(super) domain: String,
    pub(super) source_id: String,
    pub(super) reader_profile: String,
    pub(super) baseline_id: String,
    pub(super) baseline_generation: u64,
    pub(super) seal_digest: String,
    pub(super) workspace_name: String,
    pub(super) attachment_id: String,
    pub(super) locator: WorkspaceLocator,
    pub(super) workspace_binding: WorkspaceBinding,
    pub(super) selected_checkout: SelectedCheckout,
}
