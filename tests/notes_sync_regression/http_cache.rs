use super::*;

#[test]
fn notes_sync_http_backend_clone_warms_notes_cache() {
    let server = ReferenceServer::start("127.0.0.1:0").expect("start notes reference server");
    let backend_url = server.base_url();

    let source = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_NOTES_BACKEND_KIND", "http"),
        ("GIT_AI_NOTES_BACKEND_URL", backend_url.as_str()),
        ("GIT_AI_API_KEY", "notes-sync-http-clone-test-key"),
    ]);
    let notes_db_path = source
        .test_home_path()
        .join(".git-ai")
        .join("internal")
        .join("notes-db");
    let upstream = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    let upstream_str = upstream.path().to_string_lossy().to_string();

    source
        .git_og(&["remote", "add", "origin", upstream_str.as_str()])
        .expect("add origin should succeed");
    fs::write(source.path().join("http-clone-seed.txt"), "seed\n")
        .expect("failed to write HTTP clone seed file");
    source
        .git_og(&["add", "http-clone-seed.txt"])
        .expect("add should succeed");
    source
        .git_og(&["commit", "-m", "HTTP clone seed commit"])
        .expect("seed commit should succeed");
    source
        .git_og(&["push", "-u", "origin", "HEAD"])
        .expect("initial push should succeed");

    let seed_sha = source
        .git_og(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();
    let remote_note = "http-clone-seed-note".to_string();
    server.store().put(seed_sha.clone(), remote_note.clone());

    {
        let db = NotesDatabase::open_at_path(&notes_db_path).expect("open notes db");
        assert_eq!(
            db.get_note(&seed_sha).expect("read note before clone"),
            None,
            "HTTP notes cache should be empty before clone"
        );
    }

    let clone_dir = unique_temp_path("notes-sync-http-clone");
    let clone_dir_str = clone_dir.to_string_lossy().to_string();
    let _ = fs::remove_dir_all(&clone_dir);
    source
        .git(&["clone", upstream_str.as_str(), clone_dir_str.as_str()])
        .expect("clone should succeed");

    let db = NotesDatabase::open_at_path(&notes_db_path).expect("open notes db after clone");
    assert_eq!(
        db.get_note(&seed_sha).expect("read note after clone"),
        Some(remote_note),
        "clone with HTTP notes backend should warm the local notes cache for {}",
        seed_sha
    );

    let (daemon_log_path, daemon_log) = source.daemon_diagnostics();
    assert!(
        daemon_log.contains("handling clone notes sync"),
        "daemon log should record the clone notes side effect\npath: {}\ncontents:\n{}",
        daemon_log_path.display(),
        daemon_log
    );
    assert!(
        daemon_log.contains("fetching authorship notes")
            && daemon_log.contains("backend=http")
            && daemon_log.contains("remote=origin"),
        "daemon log should record the HTTP notes fetch\npath: {}\ncontents:\n{}",
        daemon_log_path.display(),
        daemon_log
    );
}

#[test]
fn notes_sync_http_backend_plain_pull_warms_notes_cache() {
    let server = ReferenceServer::start("127.0.0.1:0").expect("start notes reference server");
    let backend_url = server.base_url();

    let local = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_NOTES_BACKEND_KIND", "http"),
        ("GIT_AI_NOTES_BACKEND_URL", backend_url.as_str()),
        ("GIT_AI_API_KEY", "notes-sync-http-pull-test-key"),
    ]);
    let notes_db_path = local
        .test_home_path()
        .join(".git-ai")
        .join("internal")
        .join("notes-db");
    let upstream = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    let default_branch = local.current_branch();
    let upstream_str = upstream.path().to_string_lossy().to_string();

    local
        .git_og(&["remote", "add", "origin", upstream_str.as_str()])
        .expect("add origin should succeed");
    fs::write(local.path().join("http-pull-base.txt"), "base\n")
        .expect("failed to write HTTP pull base file");
    local
        .git_og(&["add", "http-pull-base.txt"])
        .expect("add should succeed");
    local
        .git_og(&["commit", "-m", "base commit"])
        .expect("base commit should succeed");
    local
        .git_og(&["push", "-u", "origin", "HEAD"])
        .expect("initial push should succeed");
    local
        .git(&["config", "pull.ff", "only"])
        .expect("configure plain pull as fast-forward-only");

    let remote_clone = unique_temp_path("notes-sync-http-pull-remote");
    let remote_clone_str = remote_clone.to_string_lossy().to_string();
    let _ = fs::remove_dir_all(&remote_clone);

    run_git(&["clone", upstream_str.as_str(), remote_clone_str.as_str()]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "config",
        "user.name",
        "Test User",
    ]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "config",
        "user.email",
        "test@example.com",
    ]);

    fs::write(remote_clone.join("http-pull-remote.txt"), "remote\n")
        .expect("failed to write HTTP pull remote file");
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "add",
        "http-pull-remote.txt",
    ]);
    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "commit",
        "-m",
        "remote HTTP pull commit",
    ]);
    let remote_sha = run_git(&["-C", remote_clone_str.as_str(), "rev-parse", "HEAD"]);
    let remote_note = "http-pull-remote-note".to_string();
    server.store().put(remote_sha.clone(), remote_note.clone());

    run_git(&[
        "-C",
        remote_clone_str.as_str(),
        "push",
        "origin",
        default_branch.as_str(),
    ]);

    {
        let db = NotesDatabase::open_at_path(&notes_db_path).expect("open notes db");
        assert_eq!(
            db.get_note(&remote_sha).expect("read note before pull"),
            None,
            "HTTP notes cache should be empty before pull"
        );
    }

    local.git(&["pull"]).expect("plain pull should succeed");
    local.sync_daemon_force();

    let db = NotesDatabase::open_at_path(&notes_db_path).expect("open notes db after pull");
    assert_eq!(
        db.get_note(&remote_sha).expect("read note after pull"),
        Some(remote_note),
        "plain pull with HTTP notes backend should warm the local notes cache for {}",
        remote_sha
    );

    let (daemon_log_path, daemon_log) = local.daemon_diagnostics();
    assert!(
        daemon_log.contains("handling pull notes sync"),
        "daemon log should record the pull notes side effect\npath: {}\ncontents:\n{}",
        daemon_log_path.display(),
        daemon_log
    );
    assert!(
        daemon_log.contains("fetching authorship notes")
            && daemon_log.contains("backend=http")
            && daemon_log.contains("remote=origin"),
        "daemon log should record the HTTP notes fetch\npath: {}\ncontents:\n{}",
        daemon_log_path.display(),
        daemon_log
    );
}
