//! Descriptor-relative directory primitives; traversal policy belongs to callers.

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, RawFd};

pub(crate) fn open_directory_at(directory_fd: RawFd, name: &std::ffi::CStr) -> io::Result<File> {
    let descriptor = unsafe {
        libc::openat(
            directory_fd,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

pub(crate) struct DirectoryEntries {
    stream: *mut libc::DIR,
}

impl DirectoryEntries {
    pub(crate) fn open(directory_fd: RawFd) -> io::Result<Self> {
        Self::open_with(directory_fd, |descriptor| unsafe {
            libc::fdopendir(descriptor)
        })
    }

    fn open_with(
        directory_fd: RawFd,
        fdopendir: impl FnOnce(RawFd) -> *mut libc::DIR,
    ) -> io::Result<Self> {
        // A separate open-file description gives each stream its own offset.
        let directory = open_directory_at(directory_fd, c".")?;
        let stream = fdopendir(directory.as_raw_fd());
        if stream.is_null() {
            return Err(io::Error::last_os_error());
        }
        // Only successful fdopendir transfers ownership to closedir.
        let _ = directory.into_raw_fd();
        Ok(Self { stream })
    }

    /// Makes one raw call, including dots, EOF and errors; callers bound scanning.
    pub(crate) fn next_raw_name(&mut self) -> io::Result<Option<CString>> {
        self.next_raw_name_with(|stream| unsafe { libc::readdir(stream) })
    }

    fn next_raw_name_with(
        &mut self,
        readdir: impl FnOnce(*mut libc::DIR) -> *mut libc::dirent,
    ) -> io::Result<Option<CString>> {
        clear_errno();
        let entry = readdir(self.stream);
        if entry.is_null() {
            let errno = current_errno();
            return if errno == 0 {
                Ok(None)
            } else {
                Err(io::Error::from_raw_os_error(errno))
            };
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        Ok(Some(name.to_owned()))
    }
}

impl Drop for DirectoryEntries {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.stream);
        }
    }
}

#[cfg(target_os = "macos")]
fn clear_errno() {
    unsafe {
        *libc::__error() = 0;
    }
}

#[cfg(target_os = "macos")]
fn current_errno() -> libc::c_int {
    unsafe { *libc::__error() }
}

#[cfg(target_os = "linux")]
fn clear_errno() {
    unsafe {
        *libc::__errno_location() = 0;
    }
}

#[cfg(target_os = "linux")]
fn current_errno() -> libc::c_int {
    unsafe { *libc::__errno_location() }
}

#[cfg(test)]
mod tests;
