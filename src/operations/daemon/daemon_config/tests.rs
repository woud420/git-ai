use super::*;

#[test]
fn daemon_lock_reports_persistent_holder_with_compatible_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.lock");
    let _held = LockFile::try_acquire(&path).unwrap();

    let error = DaemonLock::acquire(&path)
        .err()
        .expect("held lock must exclude another daemon");
    assert_eq!(
        error.to_string(),
        "Generic error: git-ai background service is already running (lock held)"
    );
}

#[test]
fn daemon_config_rejects_lock_without_parent_as_invalid_persistence_path() {
    let mut config = DaemonConfig::from_home(Path::new("test-home"));
    config.lock_path = PathBuf::new();

    assert!(matches!(
        config.ensure_parent_dirs(),
        Err(GitAiError::Persistence(PersistenceError::Io {
            kind: std::io::ErrorKind::InvalidInput,
            ..
        }))
    ));
}

#[test]
fn test_completion_log_path_has_stable_family_hash() {
    let config = DaemonConfig::from_home(Path::new("test-home"));

    assert_eq!(
        config.test_completion_log_path_for_family("family-key"),
        PathBuf::from("test-home")
            .join(".git-ai/internal/daemon/test-completions")
            .join("02e15fa3779eb41b.jsonl")
    );
}
