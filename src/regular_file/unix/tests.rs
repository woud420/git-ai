use super::*;
use std::cell::Cell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod isolated;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod retained;

struct Fixture {
    root: tempfile::TempDir,
    directory: File,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("record"), b"original").unwrap();
        let directory = File::open(root.path()).unwrap();
        Self { root, directory }
    }

    fn path(&self) -> PathBuf {
        self.root.path().join("record")
    }

    fn read(&self, budget: &mut MetadataReadBudget, io: &mut impl FileRead) -> Result<Vec<u8>, E> {
        read_with(&self.directory, OsStr::new("record"), 32, budget, io)
    }
}

fn budget() -> MetadataReadBudget {
    MetadataReadBudget::new(9, 2, Instant::now() + Duration::from_secs(30))
}

#[derive(Clone, Copy)]
enum Mutation {
    Replace,
    Grow,
    Truncate,
    Rewrite,
}

fn mutate(path: &Path, mutation: Mutation) {
    match mutation {
        Mutation::Replace => {
            let replacement = path.with_extension("replacement");
            fs::write(&replacement, b"replaced").unwrap();
            fs::rename(&replacement, path).unwrap();
        }
        Mutation::Grow => {
            OpenOptions::new()
                .append(true)
                .open(path)
                .unwrap()
                .write_all(b"growth")
                .unwrap();
        }
        Mutation::Truncate => {
            OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap()
                .set_len(2)
                .unwrap();
        }
        Mutation::Rewrite => {
            fs::write(path, b"modified").unwrap();
            let file = OpenOptions::new().write(true).open(path).unwrap();
            file.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(7))
                .unwrap();
        }
    }
}

struct BeforeOpen {
    path: PathBuf,
}

impl FileRead for BeforeOpen {
    fn before_open(&mut self) {
        mutate(&self.path, Mutation::Replace);
    }
}

#[test]
fn named_leaf_replaced_before_open_rejects_without_charging_payload() {
    let fixture = Fixture::new();
    let mut budget = budget();
    let error = fixture
        .read(
            &mut budget,
            &mut BeforeOpen {
                path: fixture.path(),
            },
        )
        .unwrap_err();
    assert!(matches!(error, E::Changed));
    assert_eq!(budget.remaining_bytes(), 9);
    assert_eq!(budget.remaining_file_attempts(), 1);
}

struct AfterOpen {
    path: PathBuf,
    mutation: Mutation,
}

impl FileRead for AfterOpen {
    fn opened(&mut self, _: &File) {
        mutate(&self.path, self.mutation);
    }
}

#[test]
fn opened_leaf_replacement_rejects_at_relative_name_recheck() {
    let fixture = Fixture::new();
    let mut budget = budget();
    assert!(matches!(
        fixture.read(
            &mut budget,
            &mut AfterOpen {
                path: fixture.path(),
                mutation: Mutation::Replace
            }
        ),
        Err(E::Changed)
    ));
    assert_eq!(budget.remaining_bytes(), 0);
}

#[test]
fn opened_file_growth_truncation_and_same_size_write_reject() {
    for mutation in [Mutation::Grow, Mutation::Truncate, Mutation::Rewrite] {
        let fixture = Fixture::new();
        let mut budget = budget();
        assert!(matches!(
            fixture.read(
                &mut budget,
                &mut AfterOpen {
                    path: fixture.path(),
                    mutation
                }
            ),
            Err(E::Changed)
        ));
        assert_eq!(budget.remaining_bytes(), 0);
    }
}

struct AfterPayload {
    path: PathBuf,
    mutation: Mutation,
    changed: bool,
}

impl FileRead for AfterPayload {
    fn after_read(&mut self, _: &mut MetadataReadBudget) {
        if !self.changed {
            mutate(&self.path, self.mutation);
            self.changed = true;
        }
    }
}

#[test]
fn mutation_after_payload_read_does_not_return_stale_success() {
    for mutation in [
        Mutation::Replace,
        Mutation::Grow,
        Mutation::Truncate,
        Mutation::Rewrite,
    ] {
        let fixture = Fixture::new();
        let mut budget = budget();
        assert!(matches!(
            fixture.read(
                &mut budget,
                &mut AfterPayload {
                    path: fixture.path(),
                    mutation,
                    changed: false
                }
            ),
            Err(E::Changed)
        ));
        assert_eq!(budget.remaining_bytes(), 0);
    }
}

struct ReadFault {
    calls: usize,
    first_error: Option<io::ErrorKind>,
    expire_after_first: bool,
}

impl FileRead for ReadFault {
    fn read(&mut self, file: &mut File, bytes: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        if self.calls == 1
            && let Some(kind) = self.first_error
        {
            return Err(io::Error::from(kind));
        }
        file.read(bytes)
    }

    fn after_read(&mut self, budget: &mut MetadataReadBudget) {
        if self.calls == 1 && self.expire_after_first {
            budget.deadline = Instant::now();
        }
    }
}

#[test]
fn postread_deadline_and_read_error_keep_reserved_bytes_charged() {
    for first_error in [None, Some(io::ErrorKind::Other)] {
        let fixture = Fixture::new();
        let mut budget = budget();
        let mut reader = ReadFault {
            calls: 0,
            first_error,
            expire_after_first: first_error.is_none(),
        };
        let error = fixture.read(&mut budget, &mut reader).unwrap_err();
        if first_error.is_none() {
            assert!(matches!(error, E::Deadline));
        } else {
            assert!(matches!(error, E::Io(_)));
        }
        assert_eq!(reader.calls, 1);
        assert_eq!(budget.remaining_bytes(), 0);
        assert_eq!(budget.remaining_file_attempts(), 1);
    }
}

#[test]
fn interrupted_read_retries_and_uses_one_payload_reservation() {
    let fixture = Fixture::new();
    let mut budget = budget();
    let mut reader = ReadFault {
        calls: 0,
        first_error: Some(io::ErrorKind::Interrupted),
        expire_after_first: false,
    };
    assert_eq!(fixture.read(&mut budget, &mut reader).unwrap(), b"original");
    assert_eq!(reader.calls, 3);
    assert_eq!(budget.remaining_bytes(), 0);
}

#[test]
fn interrupted_read_checks_expired_deadline_before_retrying() {
    let fixture = Fixture::new();
    let mut budget = budget();
    let mut reader = ReadFault {
        calls: 0,
        first_error: Some(io::ErrorKind::Interrupted),
        expire_after_first: true,
    };
    assert!(matches!(
        fixture.read(&mut budget, &mut reader),
        Err(E::Deadline)
    ));
    assert_eq!(reader.calls, 1);
    assert_eq!(budget.remaining_bytes(), 0);
}

#[test]
fn failed_read_retry_cannot_reclaim_reserved_bytes() {
    let fixture = Fixture::new();
    let mut budget = budget();
    let mut reader = ReadFault {
        calls: 0,
        first_error: Some(io::ErrorKind::Other),
        expire_after_first: false,
    };
    assert!(matches!(
        fixture.read(&mut budget, &mut reader),
        Err(E::Io(_))
    ));
    assert!(matches!(
        fixture.read(&mut budget, &mut DirectRead),
        Err(E::ByteLimit)
    ));
    assert_eq!(budget.remaining_bytes(), 0);
    assert_eq!(budget.remaining_file_attempts(), 0);
}

#[test]
fn primitive_sets_nonblocking_cloexec_and_rejects_leaf_symlink() {
    let fixture = Fixture::new();
    let name = CString::new("record").unwrap();
    let file = open_record_at(fixture.directory.as_raw_fd(), &name).unwrap();
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    let fd_flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
    assert_ne!(flags & libc::O_NONBLOCK, 0);
    assert_ne!(fd_flags & libc::FD_CLOEXEC, 0);
    std::os::unix::fs::symlink("record", fixture.root.path().join("link")).unwrap();
    let name = CString::new("link").unwrap();
    assert!(open_record_at(fixture.directory.as_raw_fd(), &name).is_err());
}

#[test]
fn basename_length_is_bounded_before_owned_name_allocation() {
    assert!(basename(OsStr::from_bytes(&[b'x'; 255])).is_ok());
    assert!(matches!(
        basename(OsStr::from_bytes(&[b'x'; 256])),
        Err(E::InvalidName)
    ));
}

#[test]
fn basename_preserves_non_utf8_bytes_before_filesystem_access() {
    let raw = b"record-\xff";
    assert_eq!(basename(OsStr::from_bytes(raw)).unwrap().as_bytes(), raw);
}
