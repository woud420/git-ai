use crate::model::repository::checkpoint_outbox::CheckpointOutboxError;
use std::fs::File;

#[cfg(target_os = "linux")]
pub(super) fn reject_unsafe(_file: &File) -> Result<(), CheckpointOutboxError> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn clear_inherited(_file: &File) -> Result<(), CheckpointOutboxError> {
    Ok(())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use crate::unix_acl::OwnedAcl;
    use CheckpointOutboxError as E;

    pub(super) fn reject_unsafe(file: &File) -> Result<(), CheckpointOutboxError> {
        let acl =
            OwnedAcl::read(file).map_err(|error| E::from_io("inspect extended ACL", error))?;
        if acl
            .has_unsafe_allow()
            .map_err(|error| E::from_io("inspect extended ACL", error))?
        {
            return Err(CheckpointOutboxError::UnsafeReadyRecord);
        }
        Ok(())
    }

    pub(super) fn clear_inherited(file: &File) -> Result<(), CheckpointOutboxError> {
        let empty = OwnedAcl::empty().map_err(|error| E::from_io("allocate empty ACL", error))?;
        empty
            .install(file)
            .map_err(|error| E::from_io("clear inherited ACL", error))?;
        file.sync_all()
            .map_err(|error| E::from_io("sync cleared ACL", error))?;

        let current =
            OwnedAcl::read(file).map_err(|error| E::from_io("verify cleared ACL", error))?;
        if current
            .has_entries()
            .map_err(|error| E::from_io("verify cleared ACL", error))?
        {
            return Err(CheckpointOutboxError::UnsafeReadyRecord);
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub(super) fn reject_unsafe(file: &File) -> Result<(), CheckpointOutboxError> {
    macos::reject_unsafe(file)
}

#[cfg(target_os = "macos")]
pub(super) fn clear_inherited(file: &File) -> Result<(), CheckpointOutboxError> {
    macos::clear_inherited(file)
}
