use super::super::{JournalError, codec as storage_codec, invalid};
use super::bounded::ByteString;
use super::types::{
    DirectoryIdentity, MAX_NAME_BYTES, MAX_RECORD_BYTES, Platform, RegistrationRecord,
    WorkspaceRecord,
};
use crate::model::jj_observation::{
    JJ_OBSERVATION_SCHEMA_VERSION, is_root, validate_operation_id, validate_profile,
    validate_source,
};
use serde::Serialize;

pub(super) fn validate_name(name: &str) -> Result<(), JournalError> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(invalid("native registration workspace name length invalid"));
    }
    Ok(())
}

pub(super) fn decode_registration(
    raw: &[u8],
    stored_length: u64,
    checksum: &str,
) -> Result<RegistrationRecord, JournalError> {
    let record: RegistrationRecord =
        storage_codec::decode(raw, stored_length, checksum, MAX_RECORD_BYTES)?;
    if record.record_version != 1
        || record.domain != "git-ai/jj/source-registration/v1"
        || record.baseline_generation != 1
        || record.source_binding.identity_format != "unix-device-inode/v1"
    {
        return Err(invalid("native registration record contract invalid"));
    }
    for id in [
        &record.source_id,
        &record.baseline_id,
        &record.seal_digest,
        &record.initial_attachment_id,
        &record.initial_workspace_record_id,
    ] {
        validate_source(id)?;
    }
    validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, &record.reader_profile)?;
    validate_name(&record.initial_workspace_name)?;
    if storage_codec::checksum(&record.seal_bytes.0) != record.seal_digest {
        return Err(invalid("native registration seal digest mismatch"));
    }
    require_canonical(&record, raw)?;
    Ok(record)
}

pub(super) fn decode_workspace(
    raw: &[u8],
    stored_length: u64,
    checksum: &str,
) -> Result<WorkspaceRecord, JournalError> {
    let record: WorkspaceRecord =
        storage_codec::decode(raw, stored_length, checksum, MAX_RECORD_BYTES)?;
    if record.record_version != 1
        || record.domain != "git-ai/jj/workspace-attachment/v1"
        || record.baseline_generation != 1
    {
        return Err(invalid("native workspace record contract invalid"));
    }
    for id in [
        &record.source_id,
        &record.baseline_id,
        &record.seal_digest,
        &record.attachment_id,
    ] {
        validate_source(id)?;
    }
    validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, &record.reader_profile)?;
    validate_name(&record.workspace_name)?;
    if !record.locator.workspace_root.0.starts_with(b"/")
        || record.locator.workspace_root.0.contains(&0)
    {
        return Err(invalid("native workspace locator bytes invalid"));
    }
    validate_operation_id(&record.selected_checkout.operation_id)?;
    validate_operation_id(&record.selected_checkout.view_id)?;
    if is_root(&record.selected_checkout.operation_id) {
        return Err(invalid("native workspace checkout operation is root"));
    }
    require_canonical(&record, raw)?;
    Ok(record)
}

fn require_canonical(value: &impl Serialize, raw: &[u8]) -> Result<(), JournalError> {
    if storage_codec::encode(value, MAX_RECORD_BYTES)? != raw {
        return Err(invalid(
            "native registration record encoding is not canonical",
        ));
    }
    Ok(())
}

#[derive(Serialize)]
struct RootGuard<'a> {
    domain: &'static str,
    platform: &'a Platform,
    device: u64,
    inode: u64,
}

fn root_guard(
    domain: &'static str,
    platform: &Platform,
    root: &DirectoryIdentity,
) -> Result<String, JournalError> {
    let key = RootGuard {
        domain,
        platform,
        device: root.device,
        inode: root.inode,
    };
    Ok(storage_codec::checksum(&storage_codec::encode(&key, 256)?))
}

pub(super) fn source_root_key(record: &RegistrationRecord) -> Result<String, JournalError> {
    root_guard(
        "git-ai/jj/source-root-guard/v1",
        &record.source_binding.platform,
        &record.source_binding.directories[0],
    )
}

pub(super) fn workspace_root_key(record: &WorkspaceRecord) -> Result<String, JournalError> {
    root_guard(
        "git-ai/jj/workspace-root-guard/v1",
        &record.locator.platform,
        &record.workspace_binding.directories[0],
    )
}

pub(super) fn locator_key(record: &WorkspaceRecord) -> Result<String, JournalError> {
    #[derive(Serialize)]
    struct LocatorGuard<'a> {
        domain: &'static str,
        platform: &'a Platform,
        workspace_root: &'a ByteString<{ 64 * 1024 }>,
    }
    let key = LocatorGuard {
        domain: "git-ai/jj/workspace-locator/v1",
        platform: &record.locator.platform,
        workspace_root: &record.locator.workspace_root,
    };
    Ok(storage_codec::checksum(&storage_codec::encode(
        &key,
        MAX_RECORD_BYTES,
    )?))
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
