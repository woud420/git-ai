use super::{CaptureBudget, CaptureHooks, DirectoryIdentity, JjCaptureError as E};
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

pub(super) const MAX_LOCATOR_BYTES: usize = 64 * 1024;

pub(super) struct DirectoryRegistry {
    files: Vec<File>,
    identities: Vec<DirectoryIdentity>,
    edges: Vec<Edge>,
}

struct Edge {
    parent: usize,
    name: CString,
    identity: DirectoryIdentity,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EntryKind {
    Directory(DirectoryIdentity),
    File,
    Other,
}

impl DirectoryRegistry {
    pub(super) fn new(
        budget: &mut CaptureBudget,
        hooks: &mut impl CaptureHooks,
    ) -> Result<Self, E> {
        budget.begin_directory_open(hooks)?;
        let result = (|| {
            let file = hooks.open_directory(libc::AT_FDCWD, c"/");
            budget.check(hooks)?;
            let file = file.map_err(|error| E::caused("directory", error))?;
            let identity = descriptor_identity(&file, budget, hooks)?;
            Ok(Self {
                files: vec![file],
                identities: vec![identity],
                edges: Vec::new(),
            })
        })();
        if result.is_err() {
            budget.close_directories(1);
        }
        result
    }

    pub(super) fn file(&self, index: usize) -> &File {
        &self.files[index]
    }

    pub(super) fn identity(&self, index: usize) -> DirectoryIdentity {
        self.identities[index]
    }

    pub(super) fn close(&mut self, budget: &mut CaptureBudget) {
        let count = self.files.len();
        self.files.clear();
        budget.close_directories(count);
    }

    pub(super) fn walk(
        &mut self,
        start: usize,
        bytes: &[u8],
        budget: &mut CaptureBudget,
        hooks: &mut impl CaptureHooks,
    ) -> Result<usize, E> {
        validate_path(bytes)?;
        let mut current = if bytes.starts_with(b"/") { 0 } else { start };
        // Raw splitting preserves explicit dot and parent steps, unlike lexical normalization.
        for component in bytes
            .split(|byte| *byte == b'/')
            .filter(|part| !part.is_empty())
        {
            current = self.open_child(current, component, budget, hooks)?;
        }
        Ok(current)
    }

    pub(super) fn target_parent(
        &mut self,
        start: usize,
        bytes: &[u8],
        budget: &mut CaptureBudget,
        hooks: &mut impl CaptureHooks,
    ) -> Result<(usize, CString, bool), E> {
        validate_path(bytes)?;
        let mut current = if bytes.starts_with(b"/") { 0 } else { start };
        let mut parts = bytes
            .split(|byte| *byte == b'/')
            .filter(|part| !part.is_empty())
            .peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                return Ok((current, component_name(part)?, bytes.ends_with(b"/")));
            }
            current = self.open_child(current, part, budget, hooks)?;
        }
        Ok((current, CString::from(c"."), true))
    }

    pub(super) fn open_child(
        &mut self,
        parent: usize,
        bytes: &[u8],
        budget: &mut CaptureBudget,
        hooks: &mut impl CaptureHooks,
    ) -> Result<usize, E> {
        validate_component(bytes)?;
        budget.component(hooks)?;
        budget.check_edge_capacity()?;
        let name = component_name(bytes)?;
        let before = match kind_at(self.file(parent), &name, budget, hooks)? {
            Some(EntryKind::Directory(identity)) => identity,
            _ => {
                return Err(E::invalid(
                    "directory",
                    "entry is not a directory without symlinks",
                ));
            }
        };
        budget.begin_directory_open(hooks)?;
        let opened = (|| {
            let file = hooks.open_directory(self.file(parent).as_raw_fd(), &name);
            budget.check(hooks)?;
            let file = file.map_err(|error| E::caused("directory", error))?;
            let identity = descriptor_identity(&file, budget, hooks)?;
            let after = kind_at(self.file(parent), &name, budget, hooks)?;
            if identity != before || after != Some(EntryKind::Directory(identity)) {
                return Err(E::invalid(
                    "directory",
                    "changed directory identity during opening",
                ));
            }
            Ok((file, identity))
        })();
        let (file, identity) = match opened {
            Ok(value) => value,
            Err(error) => {
                budget.close_directories(1);
                return Err(error);
            }
        };
        let index = self.files.len();
        self.files.push(file);
        self.identities.push(identity);
        self.edges.push(Edge {
            parent,
            name,
            identity,
        });
        budget.retain_edge();
        Ok(index)
    }

    pub(super) fn recheck(
        &self,
        budget: &mut CaptureBudget,
        hooks: &mut impl CaptureHooks,
    ) -> Result<(), E> {
        for edge in &self.edges {
            if kind_at(self.file(edge.parent), &edge.name, budget, hooks)?
                != Some(EntryKind::Directory(edge.identity))
            {
                return Err(E::invalid("directory", "changed retained directory edge"));
            }
        }
        budget.check(hooks)
    }
}

pub(super) fn validate_locator(path: &Path) -> Result<(), E> {
    let bytes = path.as_os_str().as_bytes();
    if !bytes.starts_with(b"/") {
        return Err(E::invalid("context", "locator must be absolute"));
    }
    validate_path(bytes)
}

fn validate_path(bytes: &[u8]) -> Result<(), E> {
    if bytes.is_empty() {
        return Err(E::invalid("pointer", "empty directory path"));
    }
    if bytes.len() > MAX_LOCATOR_BYTES {
        return Err(E::invalid("directory", "raw path byte limit exceeded"));
    }
    for part in bytes
        .split(|byte| *byte == b'/')
        .filter(|part| !part.is_empty())
    {
        validate_component(part)?;
    }
    Ok(())
}

fn validate_component(bytes: &[u8]) -> Result<(), E> {
    if bytes.len() > 255 {
        return Err(E::invalid("directory", "component byte limit exceeded"));
    }
    if bytes.is_empty() || bytes.contains(&0) || bytes.contains(&b'/') {
        return Err(E::invalid("directory", "invalid component"));
    }
    Ok(())
}

pub(super) fn component_name(bytes: &[u8]) -> Result<CString, E> {
    validate_component(bytes)?;
    CString::new(bytes).map_err(|error| E::caused("directory", error))
}

pub(super) fn kind_at(
    parent: &File,
    name: &CStr,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<Option<EntryKind>, E> {
    budget.check(hooks)?;
    let result = stat_at(parent.as_raw_fd(), name);
    budget.check(hooks)?;
    match result {
        Ok(stat) => Ok(Some(match stat.st_mode & libc::S_IFMT {
            libc::S_IFDIR => EntryKind::Directory(stat_identity(&stat)),
            libc::S_IFREG => EntryKind::File,
            _ => EntryKind::Other,
        })),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(E::caused("directory", error)),
    }
}

fn stat_at(parent: RawFd, name: &CStr) -> io::Result<libc::stat> {
    let mut stat = MaybeUninit::uninit();
    if unsafe {
        libc::fstatat(
            parent,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { stat.assume_init() })
}

fn descriptor_identity(
    file: &File,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<DirectoryIdentity, E> {
    budget.check(hooks)?;
    let metadata = file.metadata();
    budget.check(hooks)?;
    let metadata = metadata.map_err(|error| E::caused("directory", error))?;
    if !metadata.is_dir() {
        return Err(E::invalid(
            "directory",
            "opened descriptor is not a directory",
        ));
    }
    Ok(DirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

// libc dev_t differs in width between Linux and macOS; match MetadataExt's casts.
#[allow(clippy::unnecessary_cast)]
fn stat_identity(stat: &libc::stat) -> DirectoryIdentity {
    DirectoryIdentity {
        device: stat.st_dev as u64,
        inode: stat.st_ino as u64,
    }
}
