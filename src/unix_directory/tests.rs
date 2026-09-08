use super::{DirectoryEntries, open_directory_at};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs::{self, File};
use std::io;
use std::os::fd::AsRawFd;

mod isolated;

struct Fixture {
    root: tempfile::TempDir,
    directory: File,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("alpha"), b"alpha").unwrap();
        fs::write(root.path().join("beta"), b"beta").unwrap();
        let directory = File::open(root.path()).unwrap();
        Self { root, directory }
    }

    fn stream(&self) -> DirectoryEntries {
        DirectoryEntries::open(self.directory.as_raw_fd()).unwrap()
    }
}

fn expected_names() -> BTreeSet<Vec<u8>> {
    [b".".as_slice(), b"..", b"alpha", b"beta"]
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect()
}

fn collect_raw(entries: &mut DirectoryEntries) -> BTreeSet<Vec<u8>> {
    let mut names = BTreeSet::new();
    for _ in 0..8 {
        match entries.next_raw_name().unwrap() {
            Some(name) => assert!(names.insert(name.to_bytes().to_vec())),
            None => return names,
        }
    }
    panic!("fixture enumeration did not reach EOF within its call bound");
}

fn entry(name: &[u8]) -> libc::dirent {
    let mut entry: libc::dirent = unsafe { std::mem::zeroed() };
    assert!(name.len() < entry.d_name.len());
    for (target, byte) in entry.d_name.iter_mut().zip(name) {
        *target = *byte as libc::c_char;
    }
    entry
}

fn set_errno(value: libc::c_int) {
    #[cfg(target_os = "linux")]
    unsafe {
        *libc::__errno_location() = value;
    }
    #[cfg(target_os = "macos")]
    unsafe {
        *libc::__error() = value;
    }
}

#[test]
fn independent_streams_do_not_share_offsets_or_consume_the_parent_handle() {
    let fixture = Fixture::new();
    let mut first = fixture.stream();
    let leading = first.next_raw_name().unwrap().unwrap();
    let mut second = fixture.stream();
    assert_eq!(collect_raw(&mut second), expected_names());
    let mut rest = collect_raw(&mut first);
    assert!(rest.insert(leading.to_bytes().to_vec()));
    assert_eq!(rest, expected_names());
    assert_eq!(collect_raw(&mut fixture.stream()), expected_names());
    assert!(fixture.directory.metadata().unwrap().is_dir());
}

#[test]
fn raw_enumeration_includes_dots_and_owns_names_across_later_calls() {
    let fixture = Fixture::new();
    assert_eq!(collect_raw(&mut fixture.stream()), expected_names());
    let mut entries = fixture.stream();
    let mut raw = entry(b"first");
    let saved = entries.next_raw_name_with(|_| &mut raw).unwrap().unwrap();
    raw = entry(b"second");
    let later = entries.next_raw_name_with(|_| &mut raw).unwrap().unwrap();
    assert_eq!(saved.to_bytes(), b"first");
    assert_eq!(later.to_bytes(), b"second");
}

#[test]
fn raw_enumeration_preserves_non_utf8_bytes_without_creating_a_filename() {
    let fixture = Fixture::new();
    let mut entries = fixture.stream();
    let mut raw = entry(b"name-\xff");
    assert_eq!(
        entries
            .next_raw_name_with(|_| &mut raw)
            .unwrap()
            .unwrap()
            .to_bytes(),
        b"name-\xff"
    );
}

#[test]
fn caller_can_count_every_dot_junk_and_eof_call_before_filtering() {
    let fixture = Fixture::new();
    let mut entries = fixture.stream();
    let calls = Cell::new(0);
    for name in [b".".as_slice(), b"..", b"lock", b"junk", b"alpha"] {
        let mut raw = entry(name);
        let next = entries
            .next_raw_name_with(|_| {
                calls.set(calls.get() + 1);
                &mut raw
            })
            .unwrap()
            .unwrap();
        assert_eq!(next.to_bytes(), name);
    }
    assert_eq!(calls.get(), 5);
    assert!(
        entries
            .next_raw_name_with(|_| {
                calls.set(calls.get() + 1);
                std::ptr::null_mut()
            })
            .unwrap()
            .is_none()
    );
    assert_eq!(calls.get(), 6);
}

#[test]
fn repeated_dot_entries_cannot_hide_work_inside_one_raw_call() {
    let fixture = Fixture::new();
    let mut entries = fixture.stream();
    let calls = Cell::new(0);
    let mut raw = entry(b".");
    for expected in 1..=36 {
        let name = entries
            .next_raw_name_with(|_| {
                calls.set(calls.get() + 1);
                &mut raw
            })
            .unwrap()
            .unwrap();
        assert_eq!(calls.get(), expected);
        assert_eq!(name.to_bytes(), b".");
    }
}

#[test]
fn eof_clears_old_errno_and_errors_do_not_retry_or_look_like_eof() {
    let fixture = Fixture::new();
    let mut entries = fixture.stream();
    set_errno(libc::EIO);
    assert!(
        entries
            .next_raw_name_with(|_| std::ptr::null_mut())
            .unwrap()
            .is_none()
    );
    for error in [libc::EINTR, libc::EIO] {
        let calls = Cell::new(0);
        let result = entries.next_raw_name_with(|_| {
            calls.set(calls.get() + 1);
            set_errno(error);
            std::ptr::null_mut()
        });
        assert_eq!(result.unwrap_err().raw_os_error(), Some(error));
        assert_eq!(calls.get(), 1);
    }
}

#[test]
fn directory_open_preserves_flags_and_refuses_leaf_symlinks_and_files() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.path().join("child")).unwrap();
    std::os::unix::fs::symlink("child", fixture.root.path().join("link")).unwrap();
    let child = open_directory_at(fixture.directory.as_raw_fd(), c"child").unwrap();
    assert!(child.metadata().unwrap().is_dir());
    let flags = unsafe { libc::fcntl(child.as_raw_fd(), libc::F_GETFL) };
    let fd_flags = unsafe { libc::fcntl(child.as_raw_fd(), libc::F_GETFD) };
    assert!(flags >= 0 && fd_flags >= 0);
    assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
    assert_ne!(fd_flags & libc::FD_CLOEXEC, 0);
    assert!(open_directory_at(fixture.directory.as_raw_fd(), c"link").is_err());
    assert!(open_directory_at(fixture.directory.as_raw_fd(), c"alpha").is_err());
    assert_eq!(
        fs::read(fixture.root.path().join("alpha")).unwrap(),
        b"alpha"
    );
}

#[test]
fn directory_stream_rejects_nondirectory_and_invalid_descriptors() {
    let fixture = Fixture::new();
    let file = File::open(fixture.root.path().join("alpha")).unwrap();
    assert!(DirectoryEntries::open(file.as_raw_fd()).is_err());
    assert!(DirectoryEntries::open(-1).is_err());
    assert!(open_directory_at(-1, c"child").is_err());
    assert!(file.metadata().unwrap().is_file());
}

#[test]
fn opened_directory_stays_anchored_after_rename_and_caller_parent_drop() {
    let fixture = Fixture::new();
    let original = fixture.root.path().join("original");
    let renamed = fixture.root.path().join("renamed");
    fs::create_dir(&original).unwrap();
    fs::write(original.join("retained"), []).unwrap();
    let parent = File::open(&original).unwrap();
    let mut stream = DirectoryEntries::open(parent.as_raw_fd()).unwrap();
    drop(parent);
    fs::rename(&original, &renamed).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("replacement"), []).unwrap();
    assert_eq!(
        collect_raw(&mut stream),
        [b".".as_slice(), b"..", b"retained"]
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect()
    );
}
