use super::bounded::ByteString;
use serde::{Deserialize, Serialize};

pub(crate) const MAX_RECORD_BYTES: usize = 128 * 1024;
pub(crate) const MAX_NAME_BYTES: usize = 16 * 1024;

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Platform {
    Linux,
    Macos,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectoryIdentity {
    pub(crate) device: u64,
    pub(crate) inode: u64,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceBinding {
    pub(crate) platform: Platform,
    pub(crate) identity_format: String,
    pub(crate) directories: [DirectoryIdentity; 8],
    pub(crate) backends: [ByteString<{ 16 * 1024 }>; 3],
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistrationRecord {
    pub(crate) record_version: u16,
    pub(crate) domain: String,
    pub(crate) source_id: String,
    pub(crate) reader_profile: String,
    pub(crate) baseline_id: String,
    pub(crate) baseline_generation: u64,
    pub(crate) seal_bytes: ByteString<1024>,
    pub(crate) seal_digest: String,
    pub(crate) source_binding: SourceBinding,
    pub(crate) initial_workspace_name: String,
    pub(crate) initial_attachment_id: String,
    pub(crate) initial_workspace_record_id: String,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceLocator {
    pub(crate) platform: Platform,
    pub(crate) workspace_root: ByteString<{ 64 * 1024 }>,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceBinding {
    pub(crate) directories: [DirectoryIdentity; 4],
    pub(crate) colocated: bool,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BaselineRelation {
    BaselineAnchor,
    OutsideBaseline,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectedCheckout {
    pub(crate) raw_checkout_bytes: ByteString<{ 16 * 1024 }>,
    pub(crate) operation_id: String,
    pub(crate) view_id: String,
    pub(crate) baseline_relation: BaselineRelation,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceRecord {
    pub(crate) record_version: u16,
    pub(crate) domain: String,
    pub(crate) source_id: String,
    pub(crate) reader_profile: String,
    pub(crate) baseline_id: String,
    pub(crate) baseline_generation: u64,
    pub(crate) seal_digest: String,
    pub(crate) workspace_name: String,
    pub(crate) attachment_id: String,
    pub(crate) locator: WorkspaceLocator,
    pub(crate) workspace_binding: WorkspaceBinding,
    pub(crate) selected_checkout: SelectedCheckout,
}
