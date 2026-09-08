use std::ffi::CStr;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};

pub(crate) fn create_new_file_at(
    directory: RawFd,
    name: &CStr,
    mode: libc::c_uint,
) -> io::Result<File> {
    let descriptor = unsafe {
        libc::openat(
            directory,
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode,
        )
    };
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn rename_no_replace(
    directory: RawFd,
    source: &CStr,
    destination: &CStr,
) -> io::Result<()> {
    let result = unsafe {
        libc::renameatx_np(
            directory,
            source.as_ptr(),
            directory,
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    syscall_result(result)
}

#[cfg(target_os = "linux")]
pub(crate) fn rename_no_replace(
    directory: RawFd,
    source: &CStr,
    destination: &CStr,
) -> io::Result<()> {
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            directory,
            source.as_ptr(),
            directory,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    syscall_result(result as libc::c_int)
}

pub(crate) fn write_once(file: &File, bytes: &[u8]) -> io::Result<usize> {
    let result = unsafe { libc::write(file.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result as usize)
    }
}

pub(crate) fn sync_once(file: &File) -> io::Result<()> {
    // Match Rust's platform primitive without its internal EINTR retry loop.
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) };
    #[cfg(target_os = "linux")]
    let result = unsafe { libc::fsync(file.as_raw_fd()) };
    syscall_result(result)
}

pub(crate) fn reject_any_acl(file: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    if crate::unix_acl::OwnedAcl::read(file)?.has_any_entry()? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "extended ACL is not empty",
        ));
    }
    #[cfg(target_os = "linux")]
    let _ = file;
    Ok(())
}

fn syscall_result(result: libc::c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
