#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;
use std::process::Stdio;
use std::time::{Duration, Instant};

fn commit_base(repo: &TestRepo) -> String {
    fs::write(repo.path().join("tracked.txt"), "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "tracked.txt"])
        .unwrap();
    let oid = repo.stage_all_and_commit("base").unwrap().commit_sha;
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["base".human()]);
    oid
}

fn checkpoint_replacement(repo: &TestRepo, preset: &str) {
    repo.git_ai(&["checkpoint", "human", "tracked.txt"])
        .unwrap();
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    repo.git_ai(&["checkpoint", preset, "tracked.txt"]).unwrap();
}

fn assert_untracked_recreation(repo: &TestRepo) {
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    let commit = repo
        .stage_all_and_commit("uncheckpointed recreation")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
    assert!(
        commit
            .authorship_log
            .attestations
            .iter()
            .all(|file| { file.file_path != "tracked.txt" || file.entries.is_empty() }),
        "uncheckpointed recreation must have no AI or known-human attestation"
    );
}

#[test]
fn reset_discard_same_head_ai() {
    let repo = TestRepo::new();
    let head = commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git(&["--no-replace-objects", "reset", "--hard", "HEAD"])
        .unwrap();
    assert_eq!(repo.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), head);
    assert_untracked_recreation(&repo);
}

#[test]
fn reset_discard_same_head_known_human() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_known_human");
    repo.git(&["--no-replace-objects", "reset", "--hard"])
        .unwrap();
    assert_untracked_recreation(&repo);
}

#[test]
fn reset_discard_preserves_untracked_file_evidence() {
    let repo = TestRepo::new();
    let head = commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    fs::write(repo.path().join("untracked.txt"), "surviving ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "untracked.txt"])
        .unwrap();
    repo.git(&["--no-replace-objects", "reset", "--hard", &head])
        .unwrap();
    assert_eq!(
        fs::read_to_string(repo.path().join("untracked.txt")).unwrap(),
        "surviving ai\n"
    );
    assert_untracked_recreation(&repo);
    repo.filename("untracked.txt")
        .assert_committed_lines(lines!["surviving ai".ai()]);
}

#[test]
fn reset_discard_alternate_index_preserves_default_staged_evidence() {
    let repo = TestRepo::new();
    let base = commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git(&["add", "tracked.txt"]).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let alternate = temp.path().join("alternate-index");
    let env = [("GIT_INDEX_FILE", alternate.to_str().unwrap())];
    repo.git_og_with_env(&["read-tree", &base], &env).unwrap();
    repo.git_without_test_sync_for_test(&["--no-replace-objects", "reset", "--hard", "HEAD"], &env)
        .unwrap();
    repo.sync_daemon_force();
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
    repo.commit("default staged edit survives alternate-index reset")
        .unwrap();
    assert_eq!(
        repo.git_og(&["show", "HEAD:tracked.txt"]).unwrap(),
        "discarded edit\n"
    );
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn reset_discard_skip_worktree_preserves_retained_edit() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git_og(&["update-index", "--skip-worktree", "tracked.txt"])
        .unwrap();
    repo.git(&["--no-replace-objects", "reset", "--hard", "HEAD"])
        .unwrap();
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "discarded edit\n"
    );
    repo.git_og(&["update-index", "--no-skip-worktree", "tracked.txt"])
        .unwrap();
    repo.stage_all_and_commit("retained sparse edit").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn reset_discard_v4_skip_worktree_preserves_retained_edit() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git_og(&[
        "update-index",
        "--index-version=4",
        "--skip-worktree",
        "tracked.txt",
    ])
    .unwrap();
    repo.git(&["--no-replace-objects", "reset", "--hard", "HEAD"])
        .unwrap();
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "discarded edit\n"
    );
    repo.git_og(&["update-index", "--no-skip-worktree", "tracked.txt"])
        .unwrap();
    repo.stage_all_and_commit("retained v4 sparse edit")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn reset_discard_split_index_skip_worktree_preserves_retained_edit() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git_og(&["update-index", "--skip-worktree", "tracked.txt"])
        .unwrap();
    repo.git_og(&["update-index", "--split-index"]).unwrap();
    repo.git(&["--no-replace-objects", "reset", "--hard", "HEAD"])
        .unwrap();
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "discarded edit\n"
    );
    repo.git_og(&["update-index", "--no-skip-worktree", "tracked.txt"])
        .unwrap();
    repo.stage_all_and_commit("retained split-index sparse edit")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn reset_discard_fsmonitor_preserves_retained_edit() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    let hook = repo.path().join(".git/hooks/fsmonitor-fixture");
    // Model a monitor that still reports the index entry as clean. Git itself
    // retains the edit, so a successful hard reset does not prove its removal.
    repos::write_executable_script(
        &hook,
        "#!/bin/sh\nprintf '%s\\n' \"$1\" >> .git/fsmonitor-fixture-calls\nprintf 'fixture-token\\0'\n",
    )
    .unwrap();
    // Git passes this value through a shell; native Windows backslashes would
    // become shell escapes instead of separators in the hook path.
    repo.git_og(&["config", "core.fsmonitor", ".git/hooks/fsmonitor-fixture"])
        .unwrap();
    repo.git_og(&["config", "core.fsmonitorHookVersion", "2"])
        .unwrap();
    repo.git_og(&["update-index", "--fsmonitor"]).unwrap();
    repo.git_og(&["update-index", "--fsmonitor-valid", "tracked.txt"])
        .unwrap();
    let calls = repo.path().join(".git/fsmonitor-fixture-calls");
    fs::write(&calls, "").unwrap();
    repo.git(&["--no-replace-objects", "reset", "--hard", "HEAD"])
        .unwrap();
    let calls = fs::read_to_string(calls).unwrap();
    assert!(!calls.is_empty() && calls.lines().all(|line| line == "2"));
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "discarded edit\n"
    );
    repo.git_og(&["config", "--unset", "core.fsmonitor"])
        .unwrap();
    repo.git_og(&["update-index", "--no-fsmonitor"]).unwrap();
    repo.stage_all_and_commit("retained monitored edit")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn reset_discard_collection_opt_out_preserves_existing_journal() {
    let mut repo = TestRepo::new_dedicated_daemon();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    let log = repo.current_working_logs();
    let before = fs::read(log.checkpoints_file()).unwrap();
    repo.patch_git_ai_config(|patch| patch.allowed_repositories = Some(Vec::new()));
    repo.git(&["--no-replace-objects", "reset", "--hard", "HEAD"])
        .unwrap();
    repo.sync_daemon_force();
    assert!(
        fs::read(log.checkpoints_file()).unwrap() == before,
        "disabled collection must leave existing checkpoints unchanged"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
}

#[test]
fn reset_discard_preserves_checkpoint_after_unsynchronized_reset() {
    let temp = tempfile::tempdir().unwrap();
    let gate = temp.path().join("reset-gate");
    fs::write(&gate, "hold").unwrap();
    let spec = format!("reset={}", gate.display());
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND", &spec)]);
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git_without_test_sync_for_test(&["--no-replace-objects", "reset", "--hard", "HEAD"], &[])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            Instant::now() < deadline,
            "reset side effect did not enter its gate"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    let child = repo
        .git_ai_command_without_pre_sync_for_test(
            &["checkpoint", "mock_known_human", "tracked.txt"],
            &[],
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    fs::remove_file(&gate).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let commit = repo.stage_all_and_commit("known human recreation").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".human()]);
    assert!(
        commit.authorship_log.attestations.iter().any(|file| {
            file.file_path == "tracked.txt"
                && file
                    .entries
                    .iter()
                    .any(|entry| entry.hash.starts_with("h_"))
        }),
        "the post-reset known-human checkpoint must survive"
    );
}

#[test]
fn reset_discard_sqlite_and_http_backends() {
    use git_ai::config::{NotesBackendConfig, NotesBackendKind};
    use git_ai::model::authorship_log_serialization::AuthorshipLog;
    use git_ai::model::repository::notes_db::NotesDatabase;
    use git_ai::notes::reference_server::ReferenceServer;

    let server = ReferenceServer::start("127.0.0.1:0").unwrap();
    for kind in [NotesBackendKind::Sqlite, NotesBackendKind::Http] {
        let repo = TestRepo::new_with_daemon_env_and_patch(
            &[("GIT_AI_API_KEY", "reset-backend-test-key")],
            |patch| {
                patch.notes_backend = Some(NotesBackendConfig {
                    kind,
                    backend_url: Some(server.base_url()),
                });
            },
        );
        fs::write(repo.path().join("tracked.txt"), "base\n").unwrap();
        repo.git_ai(&["checkpoint", "mock_known_human", "tracked.txt"])
            .unwrap();
        repo.git(&["add", "tracked.txt"]).unwrap();
        repo.git(&["commit", "-m", "base"]).unwrap();
        repo.filename("tracked.txt")
            .assert_committed_lines(lines!["base".human()]);
        checkpoint_replacement(&repo, "mock_ai");
        repo.git(&["--no-replace-objects", "reset", "--hard", "HEAD"])
            .unwrap();
        fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
        repo.git(&["add", "tracked.txt"]).unwrap();
        repo.git(&["commit", "-m", "uncheckpointed recreation"])
            .unwrap();
        repo.filename("tracked.txt")
            .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
        let head = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
        let db =
            NotesDatabase::open_at_path(&repo.test_home_path().join(".git-ai/internal/notes-db"))
                .unwrap();
        let note = db
            .get_note(head.trim())
            .unwrap()
            .expect("backend must persist the commit note");
        let log = AuthorshipLog::deserialize_from_string(&note).unwrap();
        assert!(log.attestations.iter().all(|file| file.entries.is_empty()));
        assert!(repo.read_authorship_note(head.trim()).is_none());
    }
}

#[test]
fn reset_discard_preserves_non_hard_modes() {
    for mode in ["--soft", "--mixed", "--keep", "--merge"] {
        let repo = TestRepo::new();
        commit_base(&repo);
        checkpoint_replacement(&repo, "mock_ai");
        repo.git(&["--no-replace-objects", "reset", mode, "HEAD"])
            .unwrap();
        repo.stage_all_and_commit(mode).unwrap();
        repo.filename("tracked.txt")
            .assert_committed_lines(lines!["discarded edit".ai()]);
    }
}

#[test]
fn reset_discard_preserves_index_only_pathspec_reset() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git(&["add", "tracked.txt"]).unwrap();
    repo.git(&["--no-replace-objects", "reset", "HEAD", "--", "tracked.txt"])
        .unwrap();
    assert!(repo.git_og(&["diff", "--cached"]).unwrap().is_empty());
    repo.stage_all_and_commit("restage after index reset")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn reset_discard_preserves_failed_reset_evidence() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    assert!(
        repo.git(&["--no-replace-objects", "reset", "--hard", "missing-ref"])
            .is_err()
    );
    repo.stage_all_and_commit("after failed reset").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn reset_discard_preserves_moving_head_behavior() {
    let repo = TestRepo::new();
    let base = commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.stage_all_and_commit("committed ai").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
    fs::write(repo.path().join("tracked.txt"), "pending ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "tracked.txt"])
        .unwrap();
    repo.git(&["--no-replace-objects", "reset", "--hard", &base])
        .unwrap();
    fs::write(repo.path().join("tracked.txt"), "pending ai\n").unwrap();
    repo.stage_all_and_commit("untracked after moving reset")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["pending ai".unattributed_human()]);
}

#[test]
fn reset_discard_unborn_head_remains_native_failure() {
    let repo = TestRepo::new();
    repo.git_og(&["symbolic-ref", "HEAD", "refs/heads/unborn"])
        .unwrap();
    fs::write(repo.path().join("tracked.txt"), "first ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "tracked.txt"])
        .unwrap();
    assert!(
        repo.git(&["--no-replace-objects", "reset", "--hard", "HEAD"])
            .is_err()
    );
    repo.stage_all_and_commit("first commit").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["first ai".ai()]);
}

#[test]
fn reset_discard_linked_worktree() {
    let repo = TestRepo::new_worktree();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git(&["--no-replace-objects", "reset", "--hard", "HEAD"])
        .unwrap();
    assert_untracked_recreation(&repo);
}

#[test]
fn reset_discard_git_process_count_does_not_grow_with_files() {
    let temp = tempfile::tempdir().unwrap();
    let mut counts = Vec::new();
    for file_count in [1, 8] {
        let log_path = temp.path().join(format!("spawns-{file_count}.log"));
        let log_path_string = log_path.to_string_lossy().to_string();
        let repo = TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", &log_path_string)]);
        let files: Vec<_> = (0..file_count).map(|i| format!("file-{i}.txt")).collect();
        for file in &files {
            fs::write(repo.path().join(file), "base\n").unwrap();
        }
        repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();
        repo.stage_all_and_commit("base files").unwrap();
        for file in &files {
            repo.filename(file)
                .assert_committed_lines(lines!["base".human()]);
            fs::write(repo.path().join(file), "discarded edit\n").unwrap();
        }
        repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
        fs::write(&log_path, "").unwrap();
        repo.git(&["--no-replace-objects", "reset", "--hard", "HEAD"])
            .unwrap();
        repo.sync_daemon_force();
        let commands = fs::read_to_string(&log_path).unwrap();
        assert_eq!(
            commands
                .lines()
                .filter(|command| *command == "cat-file")
                .count(),
            1,
            "reset daemon Git processes: {commands:?}"
        );
        counts.push(commands.lines().count());
        for file in &files {
            fs::write(repo.path().join(file), "discarded edit\n").unwrap();
        }
        repo.stage_all_and_commit("uncheckpointed files").unwrap();
        for file in &files {
            repo.filename(file)
                .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
        }
    }
    assert_eq!(
        counts[0], counts[1],
        "reset Git process count must be independent of file count"
    );
    println!("reset daemon Git process counts for 1 and 8 files: {counts:?}");
}

subdir_test_variants! {
    fn reset_discard_invocation() {
        let repo = TestRepo::new();
        commit_base(&repo);
        checkpoint_replacement(&repo, "mock_ai");
        let subdir = repo.path().join("nested");
        fs::create_dir(&subdir).unwrap();
        repo.git_from_working_dir(&subdir, &["--no-replace-objects", "reset", "--hard", "HEAD"])
            .unwrap();
        assert_untracked_recreation(&repo);
    }
}

#[test]
fn reset_discard_replacement_tree_preserves_untracked_evidence() {
    let repo = TestRepo::new();
    let head = commit_base(&repo);
    repo.git_og(&["read-tree", "--empty"]).unwrap();
    let tree = repo.git_og(&["write-tree"]).unwrap();
    let replacement = repo
        .git_og(&["commit-tree", tree.trim(), "-m", "empty replacement"])
        .unwrap();
    repo.git_og(&["replace", &head, replacement.trim()])
        .unwrap();
    repo.git_og(&["reset", "--hard", "HEAD"]).unwrap();
    assert!(
        repo.git_og(&["ls-files", "--", "tracked.txt"])
            .unwrap()
            .is_empty()
    );
    checkpoint_replacement(&repo, "mock_ai");
    let log = repo.current_working_logs();
    let before = fs::read(log.checkpoints_file()).unwrap();
    repo.git(&["reset", "--hard", "HEAD"]).unwrap();
    assert!(fs::read(log.checkpoints_file()).unwrap() == before);
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "discarded edit\n"
    );
    repo.git_og(&["replace", "-d", &head]).unwrap();
    repo.stage_all_and_commit("retained replacement-view edit")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn reset_discard_unproven_object_view_preserves_existing_boundary() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    let log = repo.current_working_logs();
    let before = fs::read(log.checkpoints_file()).unwrap();
    repo.git(&["reset", "--hard", "HEAD"]).unwrap();
    assert!(fs::read(log.checkpoints_file()).unwrap() == before);
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
}
