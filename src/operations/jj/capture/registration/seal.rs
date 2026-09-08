use super::super::JjCaptureError as E;
use super::super::budget::{CaptureBudget, CaptureHooks, DirectCapture};
use super::super::directories::stat_at;
use crate::model::jj_observation::validate_source;
use crate::regular_file::{read_regular_at, read_regular_at_retained};
use std::ffi::{CStr, OsStr};
use std::fs::{File, Metadata};
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::MetadataExt;

pub(super) mod publication;

const PREFIX: &[u8] = b"git-ai/jj/source-seal/v1\nsource_id=";
const SUFFIX: &[u8] = b"\nreader_profile=jj-simple-op-store/0.45.1\n";
const MAX_SEAL_BYTES: usize = 1024;

pub(crate) struct SourceSeal {
    source_id: String,
    bytes: Vec<u8>,
}

impl SourceSeal {
    pub(super) fn new(source: &str) -> Result<Self, E> {
        validate_source(source).map_err(|error| E::caused("seal", error))?;
        let mut bytes = Vec::with_capacity(PREFIX.len() + 64 + SUFFIX.len());
        bytes.extend_from_slice(PREFIX);
        bytes.extend_from_slice(source.as_bytes());
        bytes.extend_from_slice(SUFFIX);
        Ok(Self {
            source_id: source.to_owned(),
            bytes,
        })
    }

    pub(super) fn parse(raw: &[u8]) -> Result<Self, E> {
        if raw.len() != PREFIX.len() + 64 + SUFFIX.len()
            || !raw.starts_with(PREFIX)
            || !raw.ends_with(SUFFIX)
        {
            return Err(E::invalid("seal", "unsupported source seal framing"));
        }
        let source = std::str::from_utf8(&raw[PREFIX.len()..PREFIX.len() + 64])
            .map_err(|error| E::caused("seal", error))?;
        Self::new(source)
    }

    pub(crate) fn source_id(&self) -> &str {
        &self.source_id
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PublicationPhase {
    BeforeMutation,
    NamespaceCreated,
    TemporaryCreated,
    LeafWritten,
    LeafSynced,
    SealPublished,
    NamespaceSynced,
    RepositorySynced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SealSample {
    Initial,
    Publication,
    Final,
}

pub(super) trait SealHooks: CaptureHooks {
    fn publication_phase(&mut self, _phase: PublicationPhase) {}
    fn seal_sample(&mut self, _phase: SealSample) {}
    fn mkdir(&mut self, parent: RawFd, name: &CStr) -> io::Result<()> {
        syscall_result(unsafe { libc::mkdirat(parent, name.as_ptr(), 0o700) })
    }
    fn create(&mut self, parent: RawFd, name: &CStr) -> io::Result<File> {
        crate::unix_publication::create_new_file_at(parent, name, 0o600)
    }
    fn fchmod(&mut self, file: &File) -> io::Result<()> {
        syscall_result(unsafe { libc::fchmod(file.as_raw_fd(), 0o600) })
    }
    fn write(&mut self, file: &File, bytes: &[u8]) -> io::Result<usize> {
        crate::unix_publication::write_once(file, bytes)
    }
    fn sync(&mut self, file: &File) -> io::Result<()> {
        crate::unix_publication::sync_once(file)
    }
    fn rename(&mut self, parent: RawFd, source: &CStr, destination: &CStr) -> io::Result<()> {
        crate::unix_publication::rename_no_replace(parent, source, destination)
    }
    fn check_acl(&mut self, file: &File) -> io::Result<()> {
        crate::unix_publication::reject_any_acl(file)
    }
}

impl SealHooks for DirectCapture {}

pub(super) struct HeldSeal {
    file: File,
    pub(super) seal: SourceSeal,
    stamp: Stamp,
}

impl HeldSeal {
    pub(super) fn open(
        parent: &File,
        budget: &mut CaptureBudget,
        hooks: &mut impl SealHooks,
    ) -> Result<Self, E> {
        let directory = namespace_policy(parent, budget, hooks)?;
        sample(SealSample::Initial, budget, hooks)?;
        let opened = read_regular_at_retained(
            parent,
            OsStr::new("registration"),
            MAX_SEAL_BYTES,
            &mut budget.metadata,
        );
        budget.check(hooks)?;
        let (file, bytes) = opened.map_err(|error| E::caused("seal", error))?;
        let stamp = file_policy(&file, true, budget, hooks)?;
        check_named(parent, c"registration", &stamp, budget, hooks)?;
        let seal = SourceSeal::parse(&bytes)?;
        require_directory_unchanged(parent, &directory, budget, hooks)?;
        Ok(Self { file, seal, stamp })
    }

    pub(super) fn published(
        file: File,
        seal: SourceSeal,
        parent: &File,
        budget: &mut CaptureBudget,
        hooks: &mut impl SealHooks,
    ) -> Result<Self, E> {
        let stamp = file_policy(&file, false, budget, hooks)?;
        let held = Self { file, seal, stamp };
        held.recheck(parent, SealSample::Publication, budget, hooks)?;
        Ok(held)
    }

    pub(super) fn recheck(
        &self,
        parent: &File,
        at: SealSample,
        budget: &mut CaptureBudget,
        hooks: &mut impl SealHooks,
    ) -> Result<(), E> {
        let directory = namespace_policy(parent, budget, hooks)?;
        let before = file_policy(&self.file, true, budget, hooks)?;
        if before != self.stamp {
            return Err(E::invalid(
                "seal",
                "changed retained seal identity or metadata",
            ));
        }
        check_named(parent, c"registration", &before, budget, hooks)?;
        sample(at, budget, hooks)?;
        let read = read_regular_at(
            parent,
            OsStr::new("registration"),
            MAX_SEAL_BYTES,
            &mut budget.metadata,
        );
        budget.check(hooks)?;
        let bytes = read.map_err(|error| E::caused("seal", error))?;
        if bytes != self.seal.bytes || file_policy(&self.file, false, budget, hooks)? != before {
            return Err(E::invalid("seal", "changed source seal bytes or metadata"));
        }
        check_named(parent, c"registration", &before, budget, hooks)?;
        require_directory_unchanged(parent, &directory, budget, hooks)
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct Stamp {
    device: u64,
    inode: u64,
    length: u64,
    uid: u32,
    mode: u32,
    links: u64,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
}

impl Stamp {
    fn from_metadata(value: &Metadata) -> Self {
        Self {
            device: value.dev(),
            inode: value.ino(),
            length: value.len(),
            uid: value.uid(),
            mode: value.mode(),
            links: value.nlink(),
            mtime: value.mtime(),
            mtime_nsec: value.mtime_nsec(),
            ctime: value.ctime(),
            ctime_nsec: value.ctime_nsec(),
        }
    }
}

pub(super) fn file_policy(
    file: &File,
    acl: bool,
    budget: &CaptureBudget,
    hooks: &mut impl SealHooks,
) -> Result<Stamp, E> {
    policy(file, false, acl, budget, hooks)
}

pub(super) fn namespace_policy(
    file: &File,
    budget: &CaptureBudget,
    hooks: &mut impl SealHooks,
) -> Result<Stamp, E> {
    policy(file, true, true, budget, hooks)
}

fn policy(
    file: &File,
    directory: bool,
    acl: bool,
    budget: &CaptureBudget,
    hooks: &mut impl SealHooks,
) -> Result<Stamp, E> {
    budget.check(hooks)?;
    let result = file.metadata();
    budget.check(hooks)?;
    let metadata = result.map_err(|error| E::caused("seal metadata", error))?;
    let mode = if directory { 0o700 } else { 0o600 };
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != mode
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file() || metadata.nlink() != 1
        }
    {
        return Err(E::invalid(
            "seal",
            "unsafe namespace or seal ownership, mode, type or links",
        ));
    }
    if acl {
        let result = hooks.check_acl(file);
        budget.check(hooks)?;
        result.map_err(|error| E::caused("seal ACL", error))?;
    }
    Ok(Stamp::from_metadata(&metadata))
}

fn require_directory_unchanged(
    file: &File,
    before: &Stamp,
    budget: &CaptureBudget,
    hooks: &mut impl SealHooks,
) -> Result<(), E> {
    if &policy(file, true, false, budget, hooks)? != before {
        return Err(E::invalid(
            "seal",
            "changed namespace metadata during seal sampling",
        ));
    }
    Ok(())
}

// Match MetadataExt's identity conversion on both libc field-width profiles.
#[allow(clippy::unnecessary_cast)]
pub(super) fn check_named(
    parent: &File,
    name: &CStr,
    expected: &Stamp,
    budget: &CaptureBudget,
    hooks: &mut impl SealHooks,
) -> Result<(), E> {
    budget.check(hooks)?;
    let result = stat_at(parent.as_raw_fd(), name);
    budget.check(hooks)?;
    let named = result.map_err(|error| E::caused("seal metadata", error))?;
    if named.st_dev as u64 != expected.device
        || named.st_ino as u64 != expected.inode
        || named.st_size < 0
        || named.st_size as u64 != expected.length
        || named.st_mode as u32 != expected.mode
        || named.st_uid != expected.uid
        || named.st_nlink as u64 != expected.links
    {
        return Err(E::invalid(
            "seal",
            "changed named seal identity or metadata",
        ));
    }
    Ok(())
}

fn sample(at: SealSample, budget: &CaptureBudget, hooks: &mut impl SealHooks) -> Result<(), E> {
    hooks.seal_sample(at);
    budget.check(hooks)
}

fn syscall_result(result: libc::c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
