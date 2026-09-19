#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use git_ai::config::{NotesBackendConfig, NotesBackendKind};
use git_ai::model::repository::notes_db::NotesDatabase;
use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;

fn source() -> (TestRepo, String, String) {
    source_with_message("source")
}

fn source_with_message(message: &str) -> (TestRepo, String, String) {
    let repo = TestRepo::new();
    fs::write(repo.path().join("source.txt"), "human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "source.txt"])
        .unwrap();
    fs::write(repo.path().join("source.txt"), "human\nremote AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "source.txt"])
        .unwrap();
    let oid = repo.stage_all_and_commit(message).unwrap().commit_sha;
    repo.filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "remote AI".ai()]);
    let note = repo.read_authorship_note(&oid).unwrap();
    (repo, oid, note)
}

fn fetch_pack(local: &TestRepo, args: &[&str]) -> Result<String, String> {
    let output = local.git_without_test_sync_for_test(args, &[]);
    local.sync_daemon_force();
    output
}

fn assert_attribution(local: &TestRepo, oid: &str) {
    local.git(&["checkout", "--detach", oid]).unwrap();
    local
        .filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "remote AI".ai()]);
}

#[test]
fn fetch_pack_sync_imports_notes_without_publishing_branch_refs() {
    let (source, oid, note) = source();
    let local = TestRepo::new();
    let path = source.path().to_str().unwrap();
    let refs_before = local
        .git_og(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
            "refs/remotes",
        ])
        .unwrap();
    let output = fetch_pack(&local, &["fetch-pack", "--all", path]).unwrap();
    assert!(
        output
            .lines()
            .any(|line| line == format!("{oid} refs/heads/main"))
    );
    assert_eq!(local.read_authorship_note(&oid), Some(note.clone()));
    assert_eq!(
        local
            .git_og(&[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
                "refs/remotes"
            ])
            .unwrap(),
        refs_before
    );
    assert!(
        local
            .git_og(&["rev-parse", "--verify", "FETCH_HEAD"])
            .is_err()
    );
    assert!(!local.path().join("source.txt").exists());
    fetch_pack(&local, &["fetch-pack", "--all", path]).unwrap();
    assert_eq!(local.read_authorship_note(&oid), Some(note));
    assert_attribution(&local, &oid);
}

#[test]
fn fetch_pack_sync_preserves_conflicting_local_note() {
    let (source, oid, _) = source();
    let local = TestRepo::new();
    let path = source.path().to_str().unwrap();
    local.git_og(&["fetch-pack", "--all", path]).unwrap();
    local
        .git_og(&["notes", "--ref=ai", "add", "-m", "local authority", &oid])
        .unwrap();
    fetch_pack(&local, &["fetch-pack", "--all", path]).unwrap();
    assert_eq!(
        local.read_authorship_note(&oid).unwrap().trim(),
        "local authority"
    );
}

#[test]
fn fetch_pack_sync_missing_notes_preserves_native_success() {
    let (source, oid, _) = source();
    source
        .git_og(&["update-ref", "-d", "refs/notes/ai"])
        .unwrap();
    let local = TestRepo::new();
    let output = fetch_pack(
        &local,
        &["fetch-pack", "--all", source.path().to_str().unwrap()],
    )
    .unwrap();
    assert!(output.contains(&oid));
    assert!(local.read_authorship_note(&oid).is_none());
    local.git(&["checkout", "--detach", &oid]).unwrap();
    local.filename("source.txt").assert_committed_lines(lines![
        "human".unattributed_human(),
        "remote AI".unattributed_human()
    ]);
}

#[test]
fn fetch_pack_sync_unsupported_shapes_preserve_native_behavior() {
    let (source, oid, _) = source();
    let local = TestRepo::new();
    let path = source.path().to_str().unwrap();
    for args in [
        vec!["fetch-pack", "--all", "--no-progress", path],
        vec!["fetch-pack", path, "refs/heads/main"],
        vec!["-c", "user.name=Override", "fetch-pack", "--all", path],
    ] {
        fetch_pack(&local, &args).unwrap();
        assert!(local.read_authorship_note(&oid).is_none(), "{args:?}");
    }
}

#[test]
fn fetch_pack_sync_collection_opt_out_preserves_notes() {
    let (source, oid, _) = source();
    let local = TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.allowed_repositories = Some(Vec::new());
    });
    let output = fetch_pack(
        &local,
        &["fetch-pack", "--all", source.path().to_str().unwrap()],
    )
    .unwrap();
    assert!(output.contains(&oid));
    assert!(local.read_authorship_note(&oid).is_none());
}

#[test]
fn fetch_pack_sync_sqlite_imports_remote_notes() {
    let (source, oid, note) = source();
    let local = TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.notes_backend = Some(NotesBackendConfig {
            kind: NotesBackendKind::Sqlite,
            backend_url: None,
        });
    });
    fetch_pack(
        &local,
        &["fetch-pack", "--all", source.path().to_str().unwrap()],
    )
    .unwrap();
    assert_eq!(local.read_authorship_note(&oid), Some(note.clone()));
    assert_attribution(&local, &oid);
    let database =
        NotesDatabase::open_at_path(&local.test_home_path().join(".git-ai/internal/notes-db"))
            .unwrap();
    assert_eq!(
        database
            .get_note(&oid)
            .unwrap()
            .map(|value| value.trim().to_owned()),
        Some(note.trim().to_owned())
    );
}

#[test]
fn fetch_pack_sync_preserves_working_log_and_existing_local_notes() {
    let (source, source_oid, source_note) = source();
    let (local, local_oid, local_note) = source_with_message("local history");
    local
        .git_ai(&["checkpoint", "human", "source.txt"])
        .unwrap();
    fs::write(
        local.path().join("source.txt"),
        "human\nremote AI\nlocal pending AI\n",
    )
    .unwrap();
    local
        .git_ai(&["checkpoint", "mock_ai", "source.txt"])
        .unwrap();
    let journal = local.current_working_logs();
    let before = fs::read(journal.checkpoints_file()).unwrap();
    fetch_pack(
        &local,
        &["fetch-pack", "--all", source.path().to_str().unwrap()],
    )
    .unwrap();
    assert_eq!(local.read_authorship_note(&source_oid), Some(source_note));
    assert_eq!(local.read_authorship_note(&local_oid), Some(local_note));
    assert_eq!(
        local.git_og(&["rev-parse", "HEAD"]).unwrap().trim(),
        local_oid
    );
    assert_eq!(fs::read(journal.checkpoints_file()).unwrap(), before);
    local.stage_all_and_commit("local pending edit").unwrap();
    local.filename("source.txt").assert_committed_lines(lines![
        "human".human(),
        "remote AI".ai(),
        "local pending AI".ai()
    ]);
}

#[test]
fn fetch_pack_sync_uses_literal_source_despite_fetch_url_rewrite() {
    let (source, oid, note) = source();
    let (other, other_oid, _) = source_with_message("different repository");
    let local = TestRepo::new();
    let rewrite = format!("url.{}.insteadOf", other.path().display());
    local
        .git_og(&["config", &rewrite, source.path().to_str().unwrap()])
        .unwrap();
    let output = fetch_pack(
        &local,
        &["fetch-pack", "--all", source.path().to_str().unwrap()],
    )
    .unwrap();
    assert!(
        output
            .lines()
            .any(|line| line == format!("{oid} refs/heads/main"))
    );
    assert!(!output.contains(&other_oid));
    assert_eq!(local.read_authorship_note(&oid), Some(note));
    assert!(local.read_authorship_note(&other_oid).is_none());
    assert_attribution(&local, &oid);
}

#[test]
fn fetch_pack_sync_failed_native_transfer_preserves_local_notes() {
    let (local, oid, note) = source();
    let missing = local.path().join("missing-repository");
    assert!(fetch_pack(&local, &["fetch-pack", "--all", missing.to_str().unwrap()]).is_err());
    assert_eq!(local.read_authorship_note(&oid), Some(note));
    local
        .filename("source.txt")
        .assert_committed_lines(lines!["human".human(), "remote AI".ai()]);
}

#[test]
fn fetch_pack_sync_http_authority_does_not_import_git_notes() {
    let (source, oid, _) = source();
    let server = git_ai::notes::reference_server::ReferenceServer::start("127.0.0.1:0").unwrap();
    let url = server.base_url();
    let local = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_NOTES_BACKEND_KIND", "http"),
        ("GIT_AI_NOTES_BACKEND_URL", &url),
        ("GIT_AI_API_KEY", "fetch-pack-test-key"),
    ]);
    fetch_pack(
        &local,
        &["fetch-pack", "--all", source.path().to_str().unwrap()],
    )
    .unwrap();
    assert!(local.git_og(&["cat-file", "-e", &oid]).is_ok());
    assert!(local.read_authorship_note(&oid).is_none());
    let database =
        NotesDatabase::open_at_path(&local.test_home_path().join(".git-ai/internal/notes-db"))
            .unwrap();
    assert!(database.get_note(&oid).unwrap().is_none());
}

#[test]
fn fetch_pack_sync_metadata_lock_failure_preserves_native_success() {
    let (source, oid, note) = source();
    let local = TestRepo::new();
    let common_dir = local
        .git_og(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .unwrap();
    let lock = std::path::Path::new(common_dir.trim()).join("refs/notes/ai.lock");
    fs::create_dir_all(lock.parent().unwrap()).unwrap();
    fs::write(&lock, "held by test\n").unwrap();
    let output = fetch_pack(
        &local,
        &["fetch-pack", "--all", source.path().to_str().unwrap()],
    )
    .unwrap();
    assert!(output.contains(&oid));
    assert!(local.read_authorship_note(&oid).is_none());
    let (_, diagnostics) = local.daemon_diagnostics();
    assert!(
        diagnostics.contains("best-effort fetch-pack notes sync failed"),
        "{diagnostics}"
    );
    fs::remove_file(lock).unwrap();
    fetch_pack(
        &local,
        &["fetch-pack", "--all", source.path().to_str().unwrap()],
    )
    .unwrap();
    assert_eq!(local.read_authorship_note(&oid), Some(note));
    assert_attribution(&local, &oid);
}

#[test]
fn fetch_pack_sync_keeps_internal_ref_updates_out_of_user_hooks() {
    let (source, oid, note) = source();
    for existing_note in [false, true] {
        let local = if existing_note {
            source_with_message("local hooks history").0
        } else {
            TestRepo::new()
        };
        let hooks = tempfile::tempdir().unwrap();
        let hook = hooks.path().join("reference-transaction");
        let calls = hooks.path().join("reference-transaction.calls");
        repos::write_executable_script(
            &hook,
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$0.calls\"\nexit 0\n",
        )
        .unwrap();
        let hooks_path = hooks.path().to_str().unwrap();
        local
            .git_og(&["config", "core.hooksPath", hooks_path])
            .unwrap();
        fetch_pack(
            &local,
            &["fetch-pack", "--all", source.path().to_str().unwrap()],
        )
        .unwrap();
        assert!(
            !calls.exists(),
            "internal metadata updates invoked user hooks"
        );
        assert_eq!(local.read_authorship_note(&oid), Some(note.clone()));
        fetch_pack(&local, &["update-ref", "refs/heads/hook-control", &oid]).unwrap();
        assert!(
            calls.exists(),
            "native user ref updates must still run hooks"
        );
    }
}

#[test]
fn fetch_pack_sync_process_count_does_not_grow_with_commits() {
    let mut counts = Vec::new();
    for commits in [1, 8] {
        let (source, _, _) = source();
        for number in 2..=commits {
            let line = format!("remote AI {number}");
            fs::write(source.path().join("source.txt"), format!("human\n{line}\n")).unwrap();
            source
                .git_ai(&["checkpoint", "mock_ai", "source.txt"])
                .unwrap();
            source
                .stage_all_and_commit(&format!("source {number}"))
                .unwrap();
            source
                .filename("source.txt")
                .assert_committed_lines(lines!["human".human(), line.as_str().ai()]);
        }
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("spawns.log");
        let local = TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())]);
        fs::write(&log, "").unwrap();
        fetch_pack(
            &local,
            &["fetch-pack", "--all", source.path().to_str().unwrap()],
        )
        .unwrap();
        let spawns = fs::read_to_string(&log).unwrap();
        let commands: Vec<_> = spawns.lines().collect();
        println!("fetch-pack sync Git processes for {commits} commits: {commands:?}");
        assert_eq!(
            commands
                .iter()
                .filter(|command| **command == "fetch-pack")
                .count(),
            1,
            "{spawns}"
        );
        assert!(commands.len() <= 16, "{spawns}");
        counts.push(commands.len());
        let oid = source.git_og(&["rev-parse", "HEAD"]).unwrap();
        assert_eq!(
            local.read_authorship_note(oid.trim()),
            source.read_authorship_note(oid.trim())
        );
    }
    println!("fetch-pack sync Git process counts for 1/8 commits: {counts:?}");
    assert_eq!(counts[0], counts[1]);
}

reuse_tests_in_worktree!(
    fetch_pack_sync_imports_notes_without_publishing_branch_refs,
    fetch_pack_sync_preserves_working_log_and_existing_local_notes,
);
