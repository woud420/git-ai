#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use git_ai::config::{NotesBackendConfig, NotesBackendKind};
use git_ai::model::repository::notes_db::NotesDatabase;
use git_ai::notes::reference_server::ReferenceServer;
use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;
use std::path::PathBuf;

fn attributed_source() -> (TestRepo, String, String) {
    attributed_source_with_message("attributed source")
}

fn attributed_source_with_message(message: &str) -> (TestRepo, String, String) {
    let repo = TestRepo::new();
    fs::write(repo.path().join("source.txt"), "human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "source.txt"])
        .unwrap();
    fs::write(repo.path().join("source.txt"), "human\nremote ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "source.txt"])
        .unwrap();
    let oid = repo.stage_all_and_commit(message).unwrap().commit_sha;
    repo.filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "remote ai".ai()]);
    let note = repo.read_authorship_note(&oid).unwrap();
    (repo, oid, note)
}

fn add_remote(repo: &TestRepo, name: &str, source: &TestRepo) {
    repo.git_og(&["remote", "add", name, source.path().to_str().unwrap()])
        .unwrap();
}

fn run_fetch(repo: &TestRepo, args: &[&str]) -> Result<String, String> {
    // TestRepo::git adds a -c correlation marker; exercise the production argv
    // boundary without that override and wait for the family explicitly.
    let result = repo.git_without_test_sync_for_test(args, &[]);
    repo.sync_daemon_force();
    result
}

fn assert_fetched_attribution(repo: &TestRepo, oid: &str) {
    assert_eq!(
        repo.git_og(&["rev-parse", "FETCH_HEAD"]).unwrap().trim(),
        oid
    );
    repo.git(&["checkout", "--detach", oid]).unwrap();
    repo.filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "remote ai".ai()]);
}

#[test]
fn fetch_sync_imports_selected_remote_and_preserves_local_notes() {
    let (source, oid, note) = attributed_source();
    let (other, other_oid, _) = attributed_source_with_message("other remote source");
    let local = TestRepo::new();
    fs::write(local.path().join("local.txt"), "local human\n").unwrap();
    local
        .git_ai(&["checkpoint", "mock_known_human", "local.txt"])
        .unwrap();
    let local_oid = local
        .stage_all_and_commit("local history")
        .unwrap()
        .commit_sha;
    local
        .filename("local.txt")
        .assert_committed_lines(lines!["local human".human()]);
    let local_note = local.read_authorship_note(&local_oid).unwrap();
    add_remote(&local, "upstream", &source);
    add_remote(&local, "other", &other);

    assert!(local.read_authorship_note(&oid).is_none());
    run_fetch(&local, &["fetch", "upstream"]).unwrap();
    assert_eq!(local.read_authorship_note(&oid), Some(note.clone()));
    assert_eq!(
        local.read_authorship_note(&local_oid),
        Some(local_note.clone())
    );
    assert!(
        local
            .git_og(&["rev-parse", "--verify", "refs/remotes/other/main"])
            .is_err()
    );
    assert_ne!(other_oid, oid);
    assert!(local.read_authorship_note(&other_oid).is_none());
    assert_eq!(
        local.git_og(&["rev-parse", "HEAD"]).unwrap().trim(),
        local_oid
    );
    assert!(
        local
            .git_og(&["status", "--porcelain"])
            .unwrap()
            .trim()
            .is_empty()
    );

    run_fetch(&local, &["fetch", "upstream"]).unwrap();
    assert_eq!(local.read_authorship_note(&oid), Some(note));
    assert_eq!(local.read_authorship_note(&local_oid), Some(local_note));
    assert_fetched_attribution(&local, &oid);
}

#[test]
fn fetch_sync_preserves_existing_note_on_conflict() {
    let (source, oid, _) = attributed_source();
    let local = TestRepo::new();
    add_remote(&local, "upstream", &source);
    local.git_og(&["fetch", "upstream"]).unwrap();
    local
        .git_og(&["notes", "--ref=ai", "add", "-m", "local note", &oid])
        .unwrap();

    run_fetch(&local, &["fetch", "upstream"]).unwrap();
    assert_eq!(
        local.read_authorship_note(&oid).unwrap().trim(),
        "local note"
    );
}

#[test]
fn fetch_sync_skips_unsupported_shapes_and_failed_fetches() {
    let (source, oid, _) = attributed_source();
    let local = TestRepo::new();
    add_remote(&local, "origin", &source);
    for args in [
        vec!["fetch", "--dry-run", "origin"],
        vec!["fetch", "--all"],
        vec!["fetch", "--multiple", "origin"],
        vec!["fetch", "origin", "main"],
        vec!["fetch"],
    ] {
        run_fetch(&local, &args).unwrap();
        assert!(
            local.read_authorship_note(&oid).is_none(),
            "unexpected sync for {args:?}"
        );
    }
    assert!(run_fetch(&local, &["fetch", "missing-remote"]).is_err());
    assert!(local.read_authorship_note(&oid).is_none());
}

#[test]
fn fetch_sync_tolerates_remote_without_notes() {
    let (source, oid, _) = attributed_source();
    source
        .git_og(&["update-ref", "-d", "refs/notes/ai"])
        .unwrap();
    let local = TestRepo::new();
    add_remote(&local, "upstream", &source);

    run_fetch(&local, &["fetch", "upstream"]).unwrap();
    assert!(local.read_authorship_note(&oid).is_none());
    assert_eq!(
        local.git_og(&["rev-parse", "FETCH_HEAD"]).unwrap().trim(),
        oid
    );
    local.git(&["checkout", "--detach", &oid]).unwrap();
    local.filename("source.txt").assert_committed_lines(lines![
        "human".unattributed_human(),
        "remote ai".unattributed_human()
    ]);
}

#[test]
fn fetch_sync_skips_command_scoped_remote_overrides() {
    let (source, source_oid, _) = attributed_source();
    let (other, other_oid, _) = attributed_source_with_message("overridden remote");
    let local = TestRepo::new();
    add_remote(&local, "upstream", &source);
    let rewrite = format!(
        "url.{}.insteadOf={}",
        other.path().display(),
        source.path().display()
    );

    run_fetch(&local, &["-c", &rewrite, "fetch", "upstream"]).unwrap();
    assert_eq!(
        local.git_og(&["rev-parse", "FETCH_HEAD"]).unwrap().trim(),
        other_oid
    );
    assert!(local.read_authorship_note(&source_oid).is_none());
    assert!(local.read_authorship_note(&other_oid).is_none());
}

#[test]
fn fetch_sync_note_write_failure_keeps_fetch_successful() {
    let (source, oid, note) = attributed_source();
    let local = TestRepo::new();
    add_remote(&local, "upstream", &source);
    let common_dir = local
        .git_og(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .unwrap();
    let lock_path = PathBuf::from(common_dir.trim()).join("refs/notes/ai.lock");
    fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    fs::write(&lock_path, "held by test\n").unwrap();

    run_fetch(&local, &["fetch", "upstream"]).unwrap();
    assert_eq!(
        local.git_og(&["rev-parse", "FETCH_HEAD"]).unwrap().trim(),
        oid
    );
    assert!(local.read_authorship_note(&oid).is_none());
    let (_, log) = local.daemon_diagnostics();
    assert!(
        log.contains("best-effort fetch notes sync failed"),
        "missing diagnostic: {log}"
    );

    fs::remove_file(lock_path).unwrap();
    run_fetch(&local, &["fetch", "upstream"]).unwrap();
    assert_eq!(local.read_authorship_note(&oid), Some(note));
    assert_fetched_attribution(&local, &oid);
}

#[test]
fn fetch_sync_sqlite_backend_reads_imported_git_notes() {
    let (source, oid, note) = attributed_source();
    let local = TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.notes_backend = Some(NotesBackendConfig {
            kind: NotesBackendKind::Sqlite,
            backend_url: None,
        });
    });
    add_remote(&local, "upstream", &source);

    run_fetch(&local, &["fetch", "upstream"]).unwrap();
    assert_eq!(local.read_authorship_note(&oid), Some(note.clone()));
    assert_fetched_attribution(&local, &oid);
    let db_path = local.test_home_path().join(".git-ai/internal/notes-db");
    assert_eq!(
        NotesDatabase::open_at_path(&db_path)
            .unwrap()
            .get_note(&oid)
            .unwrap()
            .map(|s| s.trim().to_owned()),
        Some(note.trim().to_owned())
    );
}

#[test]
fn fetch_sync_http_backend_warms_existing_remote_head() {
    let (source, oid, note) = attributed_source();
    let server = ReferenceServer::start("127.0.0.1:0").unwrap();
    let backend_url = server.base_url();
    server.store().put(oid.clone(), note.clone());
    let local = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_NOTES_BACKEND_KIND", "http"),
        ("GIT_AI_NOTES_BACKEND_URL", &backend_url),
        ("GIT_AI_API_KEY", "fetch-sync-test-key"),
    ]);
    add_remote(&local, "upstream", &source);
    local.git_og(&["fetch", "upstream"]).unwrap();
    local
        .git_og(&["remote", "set-head", "upstream", "--auto"])
        .unwrap();
    let db_path = local.test_home_path().join(".git-ai/internal/notes-db");
    assert_eq!(
        NotesDatabase::open_at_path(&db_path)
            .unwrap()
            .get_note(&oid)
            .unwrap(),
        None
    );

    run_fetch(&local, &["fetch", "upstream"]).unwrap();
    assert_eq!(
        NotesDatabase::open_at_path(&db_path)
            .unwrap()
            .get_note(&oid)
            .unwrap(),
        Some(note)
    );
    assert!(local.read_authorship_note(&oid).is_none());
}

#[test]
fn fetch_sync_respects_repository_collection_opt_out() {
    let (source, oid, _) = attributed_source();
    let local = TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.allowed_repositories = Some(Vec::new());
    });
    add_remote(&local, "upstream", &source);

    run_fetch(&local, &["fetch", "upstream"]).unwrap();
    assert_eq!(
        local.git_og(&["rev-parse", "FETCH_HEAD"]).unwrap().trim(),
        oid
    );
    assert!(local.read_authorship_note(&oid).is_none());
}

reuse_tests_in_worktree!(
    fetch_sync_imports_selected_remote_and_preserves_local_notes,
    fetch_sync_preserves_existing_note_on_conflict,
    fetch_sync_skips_unsupported_shapes_and_failed_fetches,
    fetch_sync_tolerates_remote_without_notes,
    fetch_sync_note_write_failure_keeps_fetch_successful,
    fetch_sync_skips_command_scoped_remote_overrides,
);
