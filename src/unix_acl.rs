use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::ptr;

type Acl = *mut libc::c_void;
type AclEntry = *mut libc::c_void;

const ACL_TYPE_EXTENDED: libc::c_int = 0x0000_0100;
const ACL_FIRST_ENTRY: libc::c_int = 0;
const ACL_NEXT_ENTRY: libc::c_int = -1;
const ACL_EXTENDED_ALLOW: libc::c_int = 1;
const ACL_EXTENDED_DENY: libc::c_int = 2;

unsafe extern "C" {
    fn acl_free(object: *mut libc::c_void) -> libc::c_int;
    fn acl_get_entry(acl: Acl, entry_id: libc::c_int, entry: *mut AclEntry) -> libc::c_int;
    fn acl_get_fd_np(fd: libc::c_int, acl_type: libc::c_int) -> Acl;
    fn acl_get_tag_type(entry: AclEntry, tag_type: *mut libc::c_int) -> libc::c_int;
    fn acl_init(count: libc::c_int) -> Acl;
    fn acl_set_fd_np(fd: libc::c_int, acl: Acl, acl_type: libc::c_int) -> libc::c_int;
}

pub(crate) struct OwnedAcl(Acl);

impl OwnedAcl {
    pub(crate) fn read(file: &File) -> io::Result<Self> {
        let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
        if acl.is_null() {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::NotFound {
                Self::empty()
            } else {
                Err(error)
            }
        } else {
            Ok(Self(acl))
        }
    }

    pub(crate) fn empty() -> io::Result<Self> {
        let acl = unsafe { acl_init(0) };
        if acl.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(acl))
        }
    }

    pub(crate) fn has_entries(&self) -> io::Result<bool> {
        self.visit_tags(|_| true)
    }

    pub(crate) fn has_unsafe_allow(&self) -> io::Result<bool> {
        self.visit_tags(|tag| tag != ACL_EXTENDED_DENY)
    }

    pub(crate) fn install(&self, file: &File) -> io::Result<()> {
        if unsafe { acl_set_fd_np(file.as_raw_fd(), self.0, ACL_TYPE_EXTENDED) } != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub(crate) fn has_any_entry(&self) -> io::Result<bool> {
        let mut entry = ptr::null_mut();
        clear_errno();
        if unsafe { acl_get_entry(self.0, ACL_FIRST_ENTRY, &mut entry) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EINVAL) {
            Ok(false)
        } else {
            Err(error)
        }
    }

    fn visit_tags(&self, reject: impl Fn(libc::c_int) -> bool) -> io::Result<bool> {
        let mut entry_id = ACL_FIRST_ENTRY;
        loop {
            let mut entry = ptr::null_mut();
            clear_errno();
            let result = unsafe { acl_get_entry(self.0, entry_id, &mut entry) };
            if result != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EINVAL) {
                    return Ok(false);
                }
                return Err(error);
            }

            let mut tag = 0;
            if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 {
                return Err(io::Error::last_os_error());
            }
            if tag == ACL_EXTENDED_ALLOW || reject(tag) {
                return Ok(true);
            }
            entry_id = ACL_NEXT_ENTRY;
        }
    }
}

impl Drop for OwnedAcl {
    fn drop(&mut self) {
        unsafe {
            acl_free(self.0);
        }
    }
}

fn clear_errno() {
    unsafe {
        *libc::__error() = 0;
    }
}
