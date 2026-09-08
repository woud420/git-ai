use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::regular_file::{MetadataReadBudget, MetadataReadError, read_regular_at};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::time::{Duration, Instant};

fn fixture() -> TestRepo {
    TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon)
}

fn budget(bytes: usize, attempts: usize) -> MetadataReadBudget {
    MetadataReadBudget::new(bytes, attempts, Instant::now() + Duration::from_secs(30))
}

#[cfg(unix)]
#[test]
fn regular_file_exact_and_empty_reads_preserve_test_repo_contents() {
    let repo = fixture();
    let parent = File::open(repo.path()).unwrap();
    fs::write(repo.path().join("record"), b"native metadata\0\xff").unwrap();
    fs::write(repo.path().join("empty"), []).unwrap();
    let before = crate::debug_context::snapshot(repo.path());
    let mut budget = budget(20, 2);
    assert_eq!(
        read_regular_at(&parent, OsStr::new("record"), 18, &mut budget).unwrap(),
        b"native metadata\0\xff"
    );
    assert!(
        read_regular_at(&parent, OsStr::new("empty"), 0, &mut budget)
            .unwrap()
            .is_empty()
    );
    assert_eq!(budget.remaining_file_attempts(), 0);
    assert_eq!(budget.remaining_bytes(), 1);
    assert_eq!(crate::debug_context::snapshot(repo.path()), before);
}

#[cfg(unix)]
#[test]
fn regular_file_per_file_payload_limit_is_inclusive() {
    let repo = fixture();
    let parent = File::open(repo.path()).unwrap();
    fs::write(repo.path().join("record"), [1, 2, 3, 4]).unwrap();
    assert_eq!(
        read_regular_at(&parent, OsStr::new("record"), 4, &mut budget(5, 1)).unwrap(),
        [1, 2, 3, 4]
    );
    let mut smaller = budget(5, 1);
    assert!(matches!(
        read_regular_at(&parent, OsStr::new("record"), 3, &mut smaller),
        Err(MetadataReadError::ByteLimit)
    ));
    assert_eq!(smaller.remaining_bytes(), 5);
    assert_eq!(smaller.remaining_file_attempts(), 0);
}

#[cfg(unix)]
#[test]
fn regular_file_aggregate_reserves_payload_and_one_eof_per_file() {
    let repo = fixture();
    let parent = File::open(repo.path()).unwrap();
    fs::write(repo.path().join("first"), [1, 2, 3]).unwrap();
    fs::write(repo.path().join("second"), [4, 5]).unwrap();
    let mut exact = budget(7, 2);
    for name in ["first", "second"] {
        read_regular_at(&parent, OsStr::new(name), 3, &mut exact).unwrap();
    }
    assert_eq!(exact.remaining_bytes(), 0);
    let mut short = budget(6, 2);
    read_regular_at(&parent, OsStr::new("first"), 3, &mut short).unwrap();
    assert!(matches!(
        read_regular_at(&parent, OsStr::new("second"), 3, &mut short),
        Err(MetadataReadError::ByteLimit)
    ));
    assert_eq!(short.remaining_bytes(), 2);
    assert_eq!(short.remaining_file_attempts(), 0);
}

#[cfg(unix)]
#[test]
fn regular_file_empty_reads_still_consume_a_byte_permit_and_attempt() {
    let repo = fixture();
    let parent = File::open(repo.path()).unwrap();
    fs::write(repo.path().join("empty"), []).unwrap();
    let mut one_byte = budget(1, 2);
    read_regular_at(&parent, OsStr::new("empty"), 0, &mut one_byte).unwrap();
    assert!(matches!(
        read_regular_at(&parent, OsStr::new("empty"), 0, &mut one_byte),
        Err(MetadataReadError::ByteLimit)
    ));
    let mut one_attempt = budget(10, 1);
    read_regular_at(&parent, OsStr::new("empty"), 0, &mut one_attempt).unwrap();
    assert!(matches!(
        read_regular_at(&parent, OsStr::new("empty"), 0, &mut one_attempt),
        Err(MetadataReadError::AttemptLimit)
    ));
    assert_eq!(one_attempt.remaining_bytes(), 9);
}

#[cfg(unix)]
#[test]
fn regular_file_failed_metadata_lookups_exhaust_attempts() {
    let repo = fixture();
    let parent = File::open(repo.path()).unwrap();
    let mut budget = budget(100, 2);
    for _ in 0..2 {
        assert!(matches!(
            read_regular_at(&parent, OsStr::new("missing"), 10, &mut budget),
            Err(MetadataReadError::Io(_))
        ));
    }
    assert!(matches!(
        read_regular_at(&parent, OsStr::new("missing"), 10, &mut budget),
        Err(MetadataReadError::AttemptLimit)
    ));
    assert_eq!(budget.remaining_bytes(), 100);
}

#[cfg(unix)]
#[test]
fn regular_file_rejects_nonempty_component_violations_before_io() {
    use std::os::unix::ffi::OsStrExt;
    let repo = fixture();
    fs::write(repo.path().join("not-a-directory"), []).unwrap();
    let invalid_parent = File::open(repo.path().join("not-a-directory")).unwrap();
    let mut budget = budget(0, 0);
    for name in [
        b"".as_slice(),
        b".",
        b"..",
        b"a/b",
        b"/absolute",
        b"trailing/",
        b"nul\0name",
    ] {
        assert!(matches!(
            read_regular_at(&invalid_parent, OsStr::from_bytes(name), 0, &mut budget),
            Err(MetadataReadError::InvalidName)
        ));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn regular_file_preserves_valid_non_utf8_basename_bytes() {
    use std::os::unix::ffi::OsStrExt;
    let repo = fixture();
    let parent = File::open(repo.path()).unwrap();
    let name = OsStr::from_bytes(b"record-\xff");
    fs::write(repo.path().join(name), [7, 8]).unwrap();
    assert_eq!(
        read_regular_at(&parent, name, 2, &mut budget(3, 1)).unwrap(),
        [7, 8]
    );
}

#[cfg(unix)]
#[test]
fn regular_file_rejects_non_directory_parent_and_directory_leaf() {
    let repo = fixture();
    fs::write(repo.path().join("record"), []).unwrap();
    let file = File::open(repo.path().join("record")).unwrap();
    assert!(matches!(
        read_regular_at(&file, OsStr::new("record"), 0, &mut budget(1, 1)),
        Err(MetadataReadError::NotDirectory)
    ));
    fs::create_dir(repo.path().join("directory")).unwrap();
    let parent = File::open(repo.path()).unwrap();
    assert!(matches!(
        read_regular_at(&parent, OsStr::new("directory"), 0, &mut budget(1, 1)),
        Err(MetadataReadError::NotRegular)
    ));
}

#[cfg(unix)]
#[test]
fn regular_file_rejects_symlinks_without_returning_target_bytes() {
    let repo = fixture();
    let parent = File::open(repo.path()).unwrap();
    fs::write(repo.path().join("target"), b"target bytes").unwrap();
    std::os::unix::fs::symlink("target", repo.path().join("link")).unwrap();
    let mut budget = budget(100, 1);
    assert!(matches!(
        read_regular_at(&parent, OsStr::new("link"), 100, &mut budget),
        Err(MetadataReadError::NotRegular)
    ));
    assert_eq!(budget.remaining_bytes(), 100);
    assert_eq!(
        fs::read(repo.path().join("target")).unwrap(),
        b"target bytes"
    );
}

#[cfg(unix)]
#[test]
fn regular_file_renamed_parent_handle_reads_original_directory() {
    let repo = fixture();
    let original = repo.path().join("metadata");
    let renamed = repo.path().join("captured-metadata");
    fs::create_dir(&original).unwrap();
    fs::write(original.join("record"), b"original").unwrap();
    let parent = File::open(&original).unwrap();
    fs::rename(&original, &renamed).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("record"), b"replacement").unwrap();
    let before = crate::debug_context::snapshot(repo.path());
    assert_eq!(
        read_regular_at(&parent, OsStr::new("record"), 20, &mut budget(21, 1)).unwrap(),
        b"original"
    );
    assert_eq!(crate::debug_context::snapshot(repo.path()), before);
}

#[cfg(unix)]
#[test]
fn regular_file_expired_deadline_rejects_before_metadata_lookup() {
    let repo = fixture();
    let parent = File::open(repo.path()).unwrap();
    let mut budget = MetadataReadBudget::new(100, 1, Instant::now());
    assert!(matches!(
        read_regular_at(&parent, OsStr::new("missing"), 100, &mut budget),
        Err(MetadataReadError::Deadline)
    ));
    assert_eq!(budget.remaining_bytes(), 100);
    assert_eq!(budget.remaining_file_attempts(), 1);
}

#[cfg(windows)]
#[test]
fn regular_file_windows_reports_unsupported_without_path_fallback() {
    let repo = fixture();
    fs::write(repo.path().join("record"), b"unchanged").unwrap();
    let parent = File::open(repo.path().join("record")).unwrap();
    let mut budget = budget(100, 1);
    assert!(matches!(
        read_regular_at(&parent, OsStr::new("missing"), 100, &mut budget),
        Err(MetadataReadError::UnsupportedPlatform)
    ));
    assert_eq!(budget.remaining_bytes(), 100);
    assert_eq!(budget.remaining_file_attempts(), 1);
    assert_eq!(fs::read(repo.path().join("record")).unwrap(), b"unchanged");
}
