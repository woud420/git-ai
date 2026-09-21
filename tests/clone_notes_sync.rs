#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;
use std::path::Path;

fn attributed_source() -> (TestRepo, String, String) {
    attributed_source_with_message("Attributed clone source")
}

fn attributed_source_with_message(message: &str) -> (TestRepo, String, String) {
    let source = TestRepo::new();
    fs::write(source.path().join("source.txt"), "human\n").unwrap();
    source
        .git_ai(&["checkpoint", "mock_known_human", "source.txt"])
        .unwrap();
    fs::write(source.path().join("source.txt"), "human\nremote AI\n").unwrap();
    source
        .git_ai(&["checkpoint", "mock_ai", "source.txt"])
        .unwrap();
    let oid = source.stage_all_and_commit(message).unwrap().commit_sha;
    source
        .filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "remote AI".ai()]);
    let note = source.read_authorship_note(&oid).unwrap();
    (source, oid, note)
}

fn clone_from(
    caller: &TestRepo,
    source: &TestRepo,
    target: &Path,
    options: &[&str],
) -> Result<String, String> {
    let mut args = vec!["clone"];
    args.extend_from_slice(options);
    args.extend([source.path().to_str().unwrap(), target.to_str().unwrap()]);
    caller.git(&args)
}

fn assert_clone_attribution(options: &[&str], remote: &str) {
    let (source, oid, note) = attributed_source();
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("cloned");
    clone_from(&source, &source, &target, options).unwrap();

    let cloned = TestRepo::new_at_path(&target);
    assert_eq!(cloned.git_og(&["remote"]).unwrap().trim(), remote);
    assert_eq!(cloned.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), oid);
    cloned
        .filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "remote AI".ai()]);
    assert_eq!(cloned.read_authorship_note(&oid), Some(note));
}

#[test]
fn clone_sync_default_remote_preserves_attribution() {
    assert_clone_attribution(&[], "origin");
}

#[test]
fn clone_sync_short_remote_name() {
    assert_clone_attribution(&["-o", "upstream"], "upstream");
}

#[test]
fn clone_sync_attached_short_remote_name() {
    assert_clone_attribution(&["-oupstream"], "upstream");
}

#[test]
fn clone_sync_long_remote_name() {
    assert_clone_attribution(&["--origin", "upstream"], "upstream");
}

#[test]
fn clone_sync_equals_remote_name() {
    assert_clone_attribution(&["--origin=upstream"], "upstream");
}

#[test]
fn clone_sync_explicit_origin_preserves_default_behavior() {
    assert_clone_attribution(&["--origin", "origin"], "origin");
}

#[test]
fn clone_sync_remote_name_is_not_hardcoded() {
    assert_clone_attribution(&["-o", "backup"], "backup");
}

#[test]
fn clone_sync_template_value_is_not_a_remote_option() {
    let (source, oid, note) = attributed_source();
    fs::create_dir(source.path().join("--origin=decoy")).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("cloned");
    clone_from(&source, &source, &target, &["--template", "--origin=decoy"]).unwrap();
    let cloned = TestRepo::new_at_path(&target);
    assert_eq!(cloned.git_og(&["remote"]).unwrap().trim(), "origin");
    cloned
        .filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "remote AI".ai()]);
    assert_eq!(cloned.read_authorship_note(&oid), Some(note));
}

#[test]
fn clone_sync_missing_notes_keeps_native_clone_success() {
    let (source, oid, _) = attributed_source();
    source
        .git_og(&["update-ref", "-d", "refs/notes/ai"])
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("cloned");
    clone_from(&source, &source, &target, &["--origin=upstream"]).unwrap();
    let cloned = TestRepo::new_at_path(&target);
    assert_eq!(cloned.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), oid);
    cloned.filename("source.txt").assert_committed_lines(lines![
        "human".unattributed_human(),
        "remote AI".unattributed_human(),
    ]);
    assert!(cloned.read_authorship_note(&oid).is_none());
}

#[test]
fn clone_sync_failed_clone_preserves_source_attribution() {
    let (source, oid, note) = attributed_source();
    let result = source.git(&[
        "clone",
        "--origin=upstream",
        "missing-source",
        "missing-target",
    ]);
    assert!(result.is_err());
    assert!(!source.path().join("missing-target").exists());
    source
        .filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "remote AI".ai()]);
    assert_eq!(source.read_authorship_note(&oid), Some(note));
}

#[test]
fn clone_sync_explicit_remote_respects_collection_opt_out() {
    let (source, oid, note) = attributed_source();
    let mut caller = TestRepo::new_dedicated_daemon();
    caller.patch_git_ai_config(|patch| patch.allowed_repositories = Some(Vec::new()));
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("cloned");
    clone_from(&caller, &source, &target, &["--origin=upstream"]).unwrap();
    let cloned = TestRepo::new_at_path(&target);
    assert_eq!(cloned.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), oid);
    cloned.filename("source.txt").assert_committed_lines(lines![
        "human".unattributed_human(),
        "remote AI".unattributed_human(),
    ]);
    assert!(cloned.read_authorship_note(&oid).is_none());
    assert_eq!(source.read_authorship_note(&oid), Some(note));
}

#[test]
fn clone_sync_http_backend_warms_explicit_remote() {
    use git_ai::model::repository::notes_db::NotesDatabase;
    use git_ai::notes::reference_server::ReferenceServer;

    let (source, oid, note) = attributed_source();
    let server = ReferenceServer::start("127.0.0.1:0").unwrap();
    let backend_url = server.base_url();
    server.store().put(oid.clone(), note.clone());
    let caller = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_NOTES_BACKEND_KIND", "http"),
        ("GIT_AI_NOTES_BACKEND_URL", &backend_url),
        ("GIT_AI_API_KEY", "clone-sync-test-key"),
    ]);
    let db_path = caller.test_home_path().join(".git-ai/internal/notes-db");
    assert_eq!(
        NotesDatabase::open_at_path(&db_path)
            .unwrap()
            .get_note(&oid)
            .unwrap(),
        None
    );
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("cloned");
    clone_from(&caller, &source, &target, &["--origin=upstream"]).unwrap();
    assert_eq!(
        NotesDatabase::open_at_path(&db_path)
            .unwrap()
            .get_note(&oid)
            .unwrap(),
        Some(note)
    );
    let cloned = TestRepo::new_at_path(&target);
    assert_eq!(cloned.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), oid);
    assert_eq!(
        fs::read_to_string(target.join("source.txt")).unwrap(),
        "human\nremote AI\n"
    );
}

#[test]
fn clone_sync_sqlite_backend_imports_explicit_remote() {
    use git_ai::config::{NotesBackendConfig, NotesBackendKind};
    use git_ai::model::repository::notes_db::NotesDatabase;

    let (source, oid, note) = attributed_source();
    let caller = TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.notes_backend = Some(NotesBackendConfig {
            kind: NotesBackendKind::Sqlite,
            backend_url: None,
        });
    });
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("cloned");
    clone_from(&caller, &source, &target, &["--origin=upstream"]).unwrap();
    let mut cloned = TestRepo::new_at_path(&target);
    cloned.patch_git_ai_config(|patch| {
        patch.notes_backend = Some(NotesBackendConfig {
            kind: NotesBackendKind::Sqlite,
            backend_url: None,
        });
    });
    assert_eq!(cloned.read_authorship_note(&oid), Some(note.clone()));
    cloned
        .filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "remote AI".ai()]);
    let db_path = cloned.test_home_path().join(".git-ai/internal/notes-db");
    assert_eq!(
        NotesDatabase::open_at_path(&db_path)
            .unwrap()
            .get_note(&oid)
            .unwrap()
            .map(|value| value.trim().to_string()),
        Some(note.trim().to_string())
    );
    assert_eq!(cloned.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), oid);
    assert_eq!(
        fs::read_to_string(target.join("source.txt")).unwrap(),
        "human\nremote AI\n"
    );
}

#[test]
fn clone_sync_command_scoped_transport_override_keeps_prior_boundary() {
    let (source, source_oid, _) = attributed_source();
    let (other, other_oid, _) = attributed_source_with_message("Overridden clone source");
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("cloned");
    let rewrite = format!(
        "url.{}.insteadOf={}",
        other.path().display(),
        source.path().display()
    );
    // The existing unsupported path reports its asynchronous origin-sync error.
    // The clone itself must still complete, without importing from the old URL.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        source.git(&[
            "-c",
            &rewrite,
            "clone",
            "--origin=upstream",
            source.path().to_str().unwrap(),
            target.to_str().unwrap(),
        ])
    }));
    assert!(outcome.is_err());
    let cloned = TestRepo::new_at_path(&target);
    assert_ne!(source_oid, other_oid);
    assert_eq!(
        cloned.git_og(&["rev-parse", "HEAD"]).unwrap().trim(),
        other_oid
    );
    cloned.filename("source.txt").assert_committed_lines(lines![
        "human".unattributed_human(),
        "remote AI".unattributed_human(),
    ]);
    assert!(cloned.read_authorship_note(&source_oid).is_none());
    assert!(cloned.read_authorship_note(&other_oid).is_none());
}

#[test]
fn clone_sync_git_process_count_does_not_grow_with_commits() {
    let mut counts = Vec::new();
    for commits in [1, 8] {
        let (source, _, _) = attributed_source();
        for number in 2..=commits {
            let ai_line = format!("remote AI {number}");
            fs::write(
                source.path().join("source.txt"),
                format!("human\n{ai_line}\n"),
            )
            .unwrap();
            source
                .git_ai(&["checkpoint", "mock_ai", "source.txt"])
                .unwrap();
            source
                .stage_all_and_commit(&format!("Source {number}"))
                .unwrap();
            source
                .filename("source.txt")
                .assert_committed_lines(lines!["human".human(), ai_line.as_str().ai(),]);
        }
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("git-spawns.log");
        let caller = TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())]);
        fs::write(&log, "").unwrap();
        let target = directory.path().join("cloned");
        clone_from(&caller, &source, &target, &["--origin=upstream"]).unwrap();
        let spawns = fs::read_to_string(&log).unwrap();
        let commands: Vec<_> = spawns.lines().collect();
        assert_eq!(
            commands
                .iter()
                .filter(|command| **command == "fetch")
                .count(),
            1
        );
        counts.push(commands.len());
        let oid = source.git_og(&["rev-parse", "HEAD"]).unwrap();
        let cloned = TestRepo::new_at_path(&target);
        assert_eq!(
            cloned.read_authorship_note(oid.trim()),
            source.read_authorship_note(oid.trim())
        );
    }
    eprintln!("clone daemon Git process counts for one/eight commits: {counts:?}");
    assert_eq!(counts[0], counts[1]);
}

#[test]
fn dedicated_daemon_readiness_does_not_leak_git_spawns() {
    let directory = tempfile::tempdir().unwrap();
    let log = directory.path().join("git-spawns.log");
    let caller = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_SPAWN_LOG", log.to_str().unwrap()),
        ("GIT_AI_TEST_DELAY_SIDE_EFFECT_MS_FOR_COMMAND", "config=250"),
    ]);

    fs::write(&log, "").unwrap();
    caller.sync_daemon_force();

    assert_eq!(fs::read_to_string(log).unwrap(), "");
}

reuse_tests_in_worktree!(
    clone_sync_default_remote_preserves_attribution,
    clone_sync_short_remote_name,
    clone_sync_attached_short_remote_name,
    clone_sync_long_remote_name,
    clone_sync_equals_remote_name,
);
