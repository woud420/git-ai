use super::*;

#[test]
fn startup_probe_waits_for_released_lock() {
    let home = tempfile::tempdir().unwrap();
    let config = DaemonConfig::from_home(home.path());
    config.ensure_parent_dirs().unwrap();
    let held = LockFile::try_acquire(&config.lock_path).unwrap();
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        drop(held);
    });

    let blocked = daemon_startup_is_blocked(&config);
    release.join().unwrap();
    assert!(!blocked, "a released lock must allow daemon startup");
    assert!(
        LockFile::try_acquire(&config.lock_path).is_some(),
        "the probe must release ownership before the daemon starts"
    );
}

#[test]
fn startup_probe_reports_persistent_lock_holder() {
    let home = tempfile::tempdir().unwrap();
    let config = DaemonConfig::from_home(home.path());
    config.ensure_parent_dirs().unwrap();
    let _held = LockFile::try_acquire(&config.lock_path).unwrap();

    assert!(daemon_startup_is_blocked(&config));
}

#[test]
fn last_lines_leave_the_file_positioned_after_the_printed_bytes() {
    use std::io::{Read, Write};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.log");
    std::fs::write(&path, "a\nb\n").unwrap();
    let mut file = std::fs::File::open(&path).unwrap();

    print_last_n_lines(&file, 1).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"c\n")
        .unwrap();

    let mut rest = String::new();
    file.read_to_string(&mut rest).unwrap();
    assert_eq!(
        rest, "c\n",
        "follow mode must resume where printing stopped"
    );
}
