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
