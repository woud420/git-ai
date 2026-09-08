use super::super::{JournalError, codec as storage_codec, invalid};
use super::bounded::ByteString;
use super::types::{
    DirectoryIdentity, MAX_NAME_BYTES, MAX_RECORD_BYTES, Platform, RegistrationRecord,
    SelectedCheckout, SourceBinding, WorkspaceBinding, WorkspaceLocator, WorkspaceRecord,
};
use crate::model::jj_observation::{
    JJ_OBSERVATION_SCHEMA_VERSION, is_root, validate_operation_id, validate_profile,
    validate_source,
};
use serde::Serialize;

pub(in crate::model::repository::jj_observation_journal) fn validate_name(
    name: &str,
) -> Result<(), JournalError> {
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
    validate_registration(&record)?;
    require_canonical(&record, raw)?;
    Ok(record)
}

pub(in crate::model::repository::jj_observation_journal) fn validate_registration(
    record: &RegistrationRecord,
) -> Result<(), JournalError> {
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
    validate_bytes(&record.seal_bytes)?;
    validate_source_binding(&record.source_binding)?;
    Ok(())
}

pub(super) fn decode_workspace(
    raw: &[u8],
    stored_length: u64,
    checksum: &str,
) -> Result<WorkspaceRecord, JournalError> {
    let record: WorkspaceRecord =
        storage_codec::decode(raw, stored_length, checksum, MAX_RECORD_BYTES)?;
    validate_workspace(&record)?;
    require_canonical(&record, raw)?;
    Ok(record)
}

pub(in crate::model::repository::jj_observation_journal) fn validate_workspace(
    record: &WorkspaceRecord,
) -> Result<(), JournalError> {
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
    validate_locator(&record.locator)?;
    validate_selected_checkout(&record.selected_checkout)?;
    Ok(())
}

pub(in crate::model::repository::jj_observation_journal) fn validate_selected_checkout(
    checkout: &SelectedCheckout,
) -> Result<(), JournalError> {
    validate_operation_id(&checkout.operation_id)?;
    validate_operation_id(&checkout.view_id)?;
    if is_root(&checkout.operation_id) {
        return Err(invalid("native workspace checkout operation is root"));
    }
    validate_bytes(&checkout.raw_checkout_bytes)?;
    Ok(())
}

pub(in crate::model::repository::jj_observation_journal) fn validate_bytes<const MAX: usize>(
    value: &ByteString<MAX>,
) -> Result<(), JournalError> {
    validate_byte_length(&value.0, MAX)
}

pub(in crate::model::repository::jj_observation_journal) fn validate_byte_length(
    bytes: &[u8],
    maximum: usize,
) -> Result<(), JournalError> {
    if bytes.is_empty() || bytes.len() > maximum {
        return Err(invalid("registration byte string length invalid"));
    }
    Ok(())
}

pub(in crate::model::repository::jj_observation_journal) fn validate_locator(
    locator: &WorkspaceLocator,
) -> Result<(), JournalError> {
    validate_bytes(&locator.workspace_root)?;
    if !locator.workspace_root.0.starts_with(b"/") || locator.workspace_root.0.contains(&0) {
        return Err(invalid("native workspace locator bytes invalid"));
    }
    Ok(())
}

pub(in crate::model::repository::jj_observation_journal) fn validate_source_binding(
    binding: &SourceBinding,
) -> Result<(), JournalError> {
    if binding.identity_format != "unix-device-inode/v1" {
        return Err(invalid("native registration record contract invalid"));
    }
    for backend in &binding.backends {
        validate_bytes(backend)?;
    }
    Ok(())
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
    source_root_guard(&record.source_binding)
}

pub(crate) fn source_root_guard(binding: &SourceBinding) -> Result<String, JournalError> {
    root_guard(
        "git-ai/jj/source-root-guard/v1",
        &binding.platform,
        &binding.directories[0],
    )
}

pub(super) fn workspace_root_key(record: &WorkspaceRecord) -> Result<String, JournalError> {
    workspace_root_guard(&record.locator, &record.workspace_binding)
}

pub(crate) fn workspace_root_guard(
    locator: &WorkspaceLocator,
    binding: &WorkspaceBinding,
) -> Result<String, JournalError> {
    root_guard(
        "git-ai/jj/workspace-root-guard/v1",
        &locator.platform,
        &binding.directories[0],
    )
}

pub(super) fn locator_key(record: &WorkspaceRecord) -> Result<String, JournalError> {
    workspace_locator_guard(&record.locator)
}

pub(crate) fn workspace_locator_guard(locator: &WorkspaceLocator) -> Result<String, JournalError> {
    validate_locator(locator)?;
    #[derive(Serialize)]
    struct LocatorGuard<'a> {
        domain: &'static str,
        platform: &'a Platform,
        workspace_root: &'a ByteString<{ 64 * 1024 }>,
    }
    let key = LocatorGuard {
        domain: "git-ai/jj/workspace-locator/v1",
        platform: &locator.platform,
        workspace_root: &locator.workspace_root,
    };
    Ok(storage_codec::checksum(&storage_codec::encode(
        &key,
        MAX_RECORD_BYTES,
    )?))
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
