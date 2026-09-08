use super::directories::{DirectoryRegistry, EntryKind, component_name, kind_at};
use super::{CaptureBudget, CaptureHooks, DirectoryIdentity, JjCaptureError as E, SampledMetadata};
use crate::regular_file::read_regular_at;
use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt;

const MAX_METADATA_BYTES: usize = 16 * 1024;
const MAX_METADATA_SAMPLES: usize = 16;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum SampleValue {
    Absent,
    Directory(DirectoryIdentity),
    File(Vec<u8>),
}

struct Sample {
    parent: usize,
    name: std::ffi::CString,
    value: SampleValue,
}

#[derive(Default)]
pub(super) struct MetadataSamples {
    entries: Vec<Sample>,
}

impl MetadataSamples {
    pub(super) fn sample(
        &mut self,
        directories: &DirectoryRegistry,
        parent: usize,
        name: &[u8],
        budget: &mut CaptureBudget,
        hooks: &mut impl CaptureHooks,
    ) -> Result<SampleValue, E> {
        if self.entries.len() >= MAX_METADATA_SAMPLES {
            return Err(E::invalid("metadata", "sample count limit exceeded"));
        }
        let name = component_name(name)?;
        let value = read_sample(directories, parent, &name, budget, hooks)?;
        self.entries.push(Sample {
            parent,
            name,
            value: value.clone(),
        });
        Ok(value)
    }

    pub(super) fn required_file(
        &mut self,
        directories: &DirectoryRegistry,
        parent: usize,
        name: &[u8],
        budget: &mut CaptureBudget,
        hooks: &mut impl CaptureHooks,
    ) -> Result<Vec<u8>, E> {
        match self.sample(directories, parent, name, budget, hooks)? {
            SampleValue::File(bytes) => Ok(bytes),
            _ => Err(E::invalid(
                "metadata",
                "required regular metadata file missing",
            )),
        }
    }

    pub(super) fn backend(
        &mut self,
        directories: &DirectoryRegistry,
        parent: usize,
        expected: &[u8],
        budget: &mut CaptureBudget,
        hooks: &mut impl CaptureHooks,
    ) -> Result<Vec<u8>, E> {
        let bytes = self.required_file(directories, parent, b"type", budget, hooks)?;
        if bytes != expected {
            return Err(E::invalid("backend", "unsupported backend"));
        }
        Ok(bytes)
    }

    pub(super) fn recheck(
        &self,
        directories: &DirectoryRegistry,
        budget: &mut CaptureBudget,
        hooks: &mut impl CaptureHooks,
    ) -> Result<(), E> {
        for sample in &self.entries {
            if read_sample(directories, sample.parent, &sample.name, budget, hooks)? != sample.value
            {
                return Err(E::invalid(
                    "metadata",
                    "changed backend, pointer or optional entry",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn into_captured(self, directories: &DirectoryRegistry) -> Vec<SampledMetadata> {
        self.entries
            .into_iter()
            .map(|sample| {
                let (bytes, directory) = match sample.value {
                    SampleValue::Absent => (None, None),
                    SampleValue::Directory(identity) => (None, Some(identity)),
                    SampleValue::File(bytes) => (Some(bytes), None),
                };
                SampledMetadata {
                    _parent: directories.identity(sample.parent),
                    _name: OsStr::from_bytes(sample.name.to_bytes()).to_owned(),
                    _bytes: bytes,
                    _directory: directory,
                }
            })
            .collect()
    }
}

fn read_sample(
    directories: &DirectoryRegistry,
    parent: usize,
    name: &CStr,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<SampleValue, E> {
    match kind_at(directories.file(parent), name, budget, hooks)? {
        None => Ok(SampleValue::Absent),
        Some(EntryKind::Directory(identity)) => Ok(SampleValue::Directory(identity)),
        Some(EntryKind::File) => {
            budget.check(hooks)?;
            let result = read_regular_at(
                directories.file(parent),
                OsStr::from_bytes(name.to_bytes()),
                MAX_METADATA_BYTES,
                &mut budget.metadata,
            );
            budget.check(hooks)?;
            result
                .map(SampleValue::File)
                .map_err(|error| E::caused("metadata", error))
        }
        Some(EntryKind::Other) => Err(E::invalid(
            "metadata",
            "entry is neither a regular file nor a directory without symlinks",
        )),
    }
}

pub(super) fn path_text(bytes: &[u8]) -> Result<&str, E> {
    let text = std::str::from_utf8(bytes).map_err(|error| E::caused("pointer", error))?;
    if text.is_empty() {
        return Err(E::invalid("pointer", "empty metadata path"));
    }
    Ok(text)
}

pub(super) fn git_pointer(bytes: &[u8]) -> Result<&str, E> {
    path_text(bytes)?
        .strip_prefix("gitdir:")
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or(E::invalid("pointer", "invalid Git directory pointer"))
}
