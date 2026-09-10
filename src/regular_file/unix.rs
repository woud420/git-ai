use super::{MetadataReadBudget, MetadataReadError as E};
use std::ffi::{CStr, CString, OsStr};
use std::fs::{File, Metadata};
use std::io::{self, Read};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;

const READ_CHUNK_BYTES: usize = 8 * 1024;
const MAX_NAME_BYTES: usize = 255;

pub(crate) fn open_record_at(directory_fd: RawFd, name: &CStr) -> io::Result<File> {
    let descriptor = unsafe {
        libc::openat(
            directory_fd,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

// Static dispatch keeps deterministic syscall interleavings private to tests.
pub(super) trait FileRead {
    fn before_open(&mut self) {}
    fn opened(&mut self, _file: &File) {}
    fn read(&mut self, file: &mut File, bytes: &mut [u8]) -> io::Result<usize> {
        file.read(bytes)
    }
    fn after_read(&mut self, _budget: &mut MetadataReadBudget) {}
}

pub(super) struct DirectRead;
impl FileRead for DirectRead {}

pub(super) fn read_with(
    parent: &File,
    name: &OsStr,
    per_file_maximum: usize,
    budget: &mut MetadataReadBudget,
    reader: &mut impl FileRead,
) -> Result<Vec<u8>, E> {
    read_retained_with(parent, name, per_file_maximum, budget, reader).map(|(_file, bytes)| bytes)
}

pub(super) fn read_retained_with(
    parent: &File,
    name: &OsStr,
    per_file_maximum: usize,
    budget: &mut MetadataReadBudget,
    reader: &mut impl FileRead,
) -> Result<(File, Vec<u8>), E> {
    let name = basename(name)?;
    budget.begin_attempt()?;
    let parent_metadata = parent.metadata();
    budget.check_deadline()?;
    if !parent_metadata?.is_dir() {
        return Err(E::NotDirectory);
    }
    let named = named_metadata(parent, &name, budget)?;
    reader.before_open();
    budget.check_deadline()?;
    let opened = open_record_at(parent.as_raw_fd(), &name);
    budget.check_deadline()?;
    let mut file = opened?;
    let initial = descriptor_metadata(&file, budget)?;
    if !initial.is_file() {
        return Err(E::NotRegular);
    }
    if !matches_named(&initial, &named) {
        return Err(E::Changed);
    }
    let length = usize::try_from(initial.len()).map_err(|_| E::ByteLimit)?;
    if length > per_file_maximum {
        return Err(E::ByteLimit);
    }
    budget.reserve(length)?;
    reader.opened(&file);
    budget.check_deadline()?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    bytes.resize(length, 0);
    let mut offset = 0;
    while offset < length {
        let end = offset + READ_CHUNK_BYTES.min(length - offset);
        let count = read_chunk(&mut file, &mut bytes[offset..end], budget, reader)?;
        if count == 0 {
            return Err(E::Changed);
        }
        offset += count;
    }
    if read_chunk(&mut file, &mut [0], budget, reader)? != 0 {
        return Err(E::Changed);
    }
    let final_metadata = descriptor_metadata(&file, budget)?;
    if !same_stamp(&initial, &final_metadata) {
        return Err(E::Changed);
    }
    let final_named = named_metadata(parent, &name, budget)?;
    if !matches_named(&initial, &final_named) {
        return Err(E::Changed);
    }
    budget.check_deadline()?;
    Ok((file, bytes))
}

fn basename(name: &OsStr) -> Result<CString, E> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes.len() > MAX_NAME_BYTES
        || bytes == b"."
        || bytes == b".."
        || bytes.contains(&b'/')
    {
        return Err(E::InvalidName);
    }
    CString::new(bytes).map_err(|_| E::InvalidName)
}

fn descriptor_metadata(file: &File, budget: &MetadataReadBudget) -> Result<Metadata, E> {
    budget.check_deadline()?;
    let result = file.metadata();
    budget.check_deadline()?;
    Ok(result?)
}

fn named_metadata(
    parent: &File,
    name: &CStr,
    budget: &MetadataReadBudget,
) -> Result<libc::stat, E> {
    budget.check_deadline()?;
    let mut metadata = MaybeUninit::uninit();
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    let error = (result < 0).then(io::Error::last_os_error);
    budget.check_deadline()?;
    if let Some(error) = error {
        return Err(error.into());
    }
    let metadata = unsafe { metadata.assume_init() };
    if metadata.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(E::NotRegular);
    }
    Ok(metadata)
}

// libc identity field widths differ across Unix targets; preserve MetadataExt's u64 casts.
#[allow(clippy::unnecessary_cast)]
fn matches_named(file: &Metadata, named: &libc::stat) -> bool {
    file.dev() == named.st_dev as u64
        && file.ino() == named.st_ino as u64
        && named.st_size >= 0
        && file.len() == named.st_size as u64
}

fn same_stamp(before: &Metadata, after: &Metadata) -> bool {
    after.is_file()
        && before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

fn read_chunk(
    file: &mut File,
    bytes: &mut [u8],
    budget: &mut MetadataReadBudget,
    reader: &mut impl FileRead,
) -> Result<usize, E> {
    loop {
        budget.check_deadline()?;
        let result = reader.read(file, bytes);
        reader.after_read(budget);
        budget.check_deadline()?;
        match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return Ok(result?),
        }
    }
}

#[cfg(test)]
mod tests;
