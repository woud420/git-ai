use super::*;
use git_ai::model::repository::lock_file::LockFile;

#[test]
fn daemon_startup_waits_for_transient_lock_holder() {
    let mut repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _ = get_binary_path();
    let lock_path = daemon_lock_path(&repo);
    fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    let held = LockFile::try_acquire(&lock_path).expect("fixture should own the daemon lock");
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(750));
        drop(held);
    });

    repo.start_dedicated_daemon_for_test();
    release.join().expect("lock release thread should complete");

    let response = send_control_request(&daemon_control_socket_path(&repo), &ControlRequest::Ping)
        .expect("daemon should accept control requests after acquiring the released lock");
    assert!(response.ok, "ping failed: {response:?}");
}
