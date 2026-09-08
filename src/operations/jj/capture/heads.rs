use super::directories::DirectoryRegistry;
use super::{CaptureBudget, CaptureHooks, JjCaptureError as E};
use crate::model::jj_observation::{is_root, validate_operation_id};
use crate::operations::jj::baseline::MAX_JJ_BASELINE_HEADS;
use crate::regular_file::read_regular_at;
use crate::unix_directory::DirectoryEntries;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::os::fd::AsRawFd;

pub(super) fn scan(
    directories: &DirectoryRegistry,
    heads: usize,
    pass: usize,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<Vec<String>, E> {
    budget.begin_directory_open(hooks)?;
    let result = (|| {
        let entries = DirectoryEntries::open(directories.file(heads).as_raw_fd());
        budget.check(hooks)?;
        let mut entries = entries.map_err(|error| E::caused("heads", error))?;
        let mut ids = BTreeSet::new();
        loop {
            budget.head_call(pass, hooks)?;
            let name = entries.next_raw_name();
            budget.check(hooks)?;
            let Some(name) = name.map_err(|error| E::caused("heads", error))? else {
                break;
            };
            if matches!(name.to_bytes(), b"." | b".." | b"lock") {
                continue;
            }
            if ids.len() >= MAX_JJ_BASELINE_HEADS {
                return Err(E::invalid("heads", "head count limit exceeded"));
            }
            let id = name.to_str().map_err(|error| E::caused("heads", error))?;
            validate_operation_id(id).map_err(|error| E::caused("heads", error))?;
            if is_root(id) {
                return Err(E::invalid("heads", "virtual root cannot be a head marker"));
            }
            if !ids.insert(id.to_owned()) {
                return Err(E::invalid("heads", "duplicate raw head name"));
            }
            budget.check(hooks)?;
            let marker = read_regular_at(
                directories.file(heads),
                OsStr::new(id),
                0,
                &mut budget.metadata,
            );
            budget.check(hooks)?;
            marker.map_err(|error| E::caused("heads", error))?;
        }
        if ids.is_empty() {
            return Err(E::invalid("heads", "empty head set"));
        }
        Ok(ids.into_iter().collect())
    })();
    // The closure drops the stream on every exit before releasing its live slot.
    budget.close_directories(1);
    result
}
