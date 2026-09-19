use super::*;
use std::time::{Duration, Instant};

#[test]
fn bounded_acquisition_waits_for_released_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.lock");
    let held = LockFile::try_acquire(&path).unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        drop(held);
    });

    let lock = LockFile::acquire_with_timeout(&path, Duration::from_secs(3));
    release.join().unwrap();
    assert!(lock.is_some(), "the released lock should become available");
    assert!(
        LockFile::try_acquire(&path).is_none(),
        "the acquired lock must remain exclusive"
    );
}

#[test]
fn bounded_acquisition_exhausts_budget_while_lock_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.lock");
    let _held = LockFile::try_acquire(&path).unwrap();
    let budget = Duration::from_millis(5);
    let started = Instant::now();

    assert!(LockFile::acquire_with_timeout(&path, budget).is_none());
    assert!(
        started.elapsed() >= budget,
        "contention must use the retry budget"
    );
}

#[test]
fn zero_budget_still_attempts_to_acquire_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.lock");
    let _held = LockFile::acquire_with_timeout(&path, Duration::ZERO)
        .expect("zero budget should allow an uncontended acquisition");

    assert!(LockFile::acquire_with_timeout(&path, Duration::ZERO).is_none());
}

#[test]
fn test_lockfile_acquire_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("test.lock");
    let lock = LockFile::try_acquire(&lock_path);
    assert!(lock.is_some(), "should acquire lock on a fresh path");
}

#[test]
fn test_lockfile_second_acquire_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("test.lock");
    let _first = LockFile::try_acquire(&lock_path).expect("first acquire should succeed");
    let second = LockFile::try_acquire(&lock_path);
    assert!(second.is_none(), "second acquire should be blocked");
}

#[test]
fn test_lockfile_released_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("test.lock");
    {
        let _lock = LockFile::try_acquire(&lock_path).expect("first acquire should succeed");
        // _lock is dropped here
    }
    let second = LockFile::try_acquire(&lock_path);
    assert!(
        second.is_some(),
        "should acquire lock after previous holder is dropped"
    );
}

#[test]
fn test_lockfile_nonexistent_parent_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("no_such_dir").join("test.lock");
    let lock = LockFile::try_acquire(&lock_path);
    assert!(
        lock.is_none(),
        "should return None when parent directory does not exist"
    );
}
