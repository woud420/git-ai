#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;
use std::process::Stdio;
use std::time::{Duration, Instant};

fn absolute_path(repo: &TestRepo, path: &str) -> String {
    let root = repo.git_og(&["rev-parse", "--show-toplevel"]).unwrap();
    format!("{}/{}", root.trim(), path)
}

fn commit_all(repo: &TestRepo, message: &str) -> String {
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "-m", message]).unwrap();
    repo.sync_daemon_force();
    repo.git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

fn prepare_discard(repo: &TestRepo, preset: &str) -> String {
    prepare_discard_at(repo, preset, "tracked.txt")
}

fn prepare_discard_at(repo: &TestRepo, preset: &str, path: &str) -> String {
    let file = repo.path().join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", path])
        .unwrap();
    let base = commit_all(repo, "base");
    repo.filename(path)
        .assert_committed_lines(lines!["base".human()]);
    fs::write(&file, "discarded edit\n").unwrap();
    repo.git_ai(&["checkpoint", preset, path]).unwrap();
    base
}

fn check_explicit_discard(preset: &str) {
    let repo = TestRepo::new();
    let base = prepare_discard(&repo, preset);
    repo.git_without_test_sync_for_test(
        &[
            "checkout",
            &base,
            "--",
            &absolute_path(&repo, "tracked.txt"),
        ],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(repo.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), base);
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
        "discarded checkpoint must not label an uncheckpointed recreation"
    );
}

#[test]
fn checkout_discard_explicit_source_ai() {
    check_explicit_discard("mock_ai");
}

#[test]
fn checkout_discard_explicit_source_known_human() {
    check_explicit_discard("mock_known_human");
}

#[test]
fn checkout_discard_fsmonitor_preserves_retained_edit() {
    let repo = TestRepo::new();
    let base = prepare_discard(&repo, "mock_ai");
    let hook = repo.path().join(".git/hooks/fsmonitor-fixture");
    // Git can retain the edit when a monitor reports its index entry as clean;
    // command success and an index write alone cannot prove it was discarded.
    repos::write_executable_script(&hook, "#!/bin/sh\nprintf 'fixture-token\\0'\n").unwrap();
    repo.git_og(&["config", "core.fsmonitor", hook.to_str().unwrap()])
        .unwrap();
    repo.git_og(&["update-index", "--fsmonitor"]).unwrap();
    repo.git_og(&["update-index", "--fsmonitor-valid", "tracked.txt"])
        .unwrap();
    repo.git_without_test_sync_for_test(
        &[
            "checkout",
            &base,
            "--",
            &absolute_path(&repo, "tracked.txt"),
        ],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
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
fn checkout_discard_without_source_preserves_staged_evidence() {
    let repo = TestRepo::new();
    prepare_discard(&repo, "mock_ai");
    repo.git(&["add", "tracked.txt"]).unwrap();
    fs::write(
        repo.path().join("tracked.txt"),
        "uncheckpointed worktree edit\n",
    )
    .unwrap();
    repo.git_without_test_sync_for_test(
        &["checkout", "--", &absolute_path(&repo, "tracked.txt")],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    repo.commit("existing staged evidence").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn checkout_discard_unsupported_forms_keep_existing_boundary() {
    for form in ["omitted-separator", "pathspec-file", "force"] {
        let repo = TestRepo::new();
        let base = prepare_discard(&repo, "mock_ai");
        let path = absolute_path(&repo, "tracked.txt");
        let temp = tempfile::tempdir().unwrap();
        let list = temp.path().join("paths");
        fs::write(&list, format!("{path}\n")).unwrap();
        let argument = format!("--pathspec-from-file={}", list.display());
        let args = match form {
            "omitted-separator" => vec!["checkout", &base, &path],
            "pathspec-file" => vec!["checkout", &base, &argument],
            "force" => vec!["checkout", "--force", &base, "--", &path],
            _ => unreachable!(),
        };
        repo.git_without_test_sync_for_test(&args, &[]).unwrap();
        repo.sync_daemon_force();
        assert_eq!(
            fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
            "base\n"
        );
        fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
        repo.stage_all_and_commit("existing unsupported boundary")
            .unwrap();
        repo.filename("tracked.txt")
            .assert_committed_lines(lines!["discarded edit".ai()]);
    }
}

#[test]
fn checkout_discard_alternate_index_preserves_default_staged_evidence() {
    let repo = TestRepo::new();
    let base = prepare_discard(&repo, "mock_ai");
    repo.git(&["add", "tracked.txt"]).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let alternate = temp.path().join("alternate-index");
    let env = [("GIT_INDEX_FILE", alternate.to_str().unwrap())];
    repo.git_og_with_env(&["read-tree", &base], &env).unwrap();
    repo.git_without_test_sync_for_test(
        &[
            "checkout",
            &base,
            "--",
            &absolute_path(&repo, "tracked.txt"),
        ],
        &env,
    )
    .unwrap();
    repo.sync_daemon_force();
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
    repo.commit("default-index edit survives alternate-index checkout")
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
fn checkout_discard_preserves_unrelated_pending_file() {
    let repo = TestRepo::new();
    let base = prepare_discard(&repo, "mock_ai");
    fs::write(repo.path().join("other.txt"), "other ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "other.txt"])
        .unwrap();
    repo.git_without_test_sync_for_test(
        &[
            "checkout",
            &base,
            "--",
            &absolute_path(&repo, "tracked.txt"),
        ],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    repo.stage_all_and_commit("recreation with unrelated edit")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
    repo.filename("other.txt")
        .assert_committed_lines(lines!["other ai".ai()]);
}

#[test]
fn checkout_discard_preserves_later_checkpoint() {
    let temp = tempfile::tempdir().unwrap();
    let gate = temp.path().join("checkout-gate");
    fs::write(&gate, "hold").unwrap();
    let spec = format!("checkout={}", gate.display());
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND", &spec)]);
    let base = prepare_discard(&repo, "mock_ai");
    repo.git_without_test_sync_for_test(
        &[
            "checkout",
            &base,
            "--",
            &absolute_path(&repo, "tracked.txt"),
        ],
        &[],
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            Instant::now() < deadline,
            "checkout did not reach its side-effect gate"
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
    let commit = repo
        .stage_all_and_commit("later known-human checkpoint")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".human()]);
    assert!(commit.authorship_log.attestations.iter().any(|file| {
        file.file_path == "tracked.txt"
            && file
                .entries
                .iter()
                .any(|entry| entry.hash.starts_with("h_"))
    }));
}

#[test]
fn checkout_discard_linked_worktree() {
    let repo = TestRepo::new_worktree();
    let base = prepare_discard(&repo, "mock_ai");
    repo.git_without_test_sync_for_test(
        &[
            "checkout",
            &base,
            "--",
            &absolute_path(&repo, "tracked.txt"),
        ],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    repo.stage_all_and_commit("linked worktree recreation")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
}

#[test]
fn checkout_discard_failed_checkout_preserves_evidence() {
    let repo = TestRepo::new();
    prepare_discard(&repo, "mock_ai");
    assert!(
        repo.git_without_test_sync_for_test(
            &[
                "checkout",
                "1111111111111111111111111111111111111111",
                "--",
                &absolute_path(&repo, "tracked.txt")
            ],
            &[]
        )
        .is_err()
    );
    repo.sync_daemon_force();
    repo.stage_all_and_commit("after failed checkout").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn checkout_discard_sparse_skip_remains_native_failure() {
    let repo = TestRepo::new();
    let base = prepare_discard(&repo, "mock_ai");
    repo.git_og(&["update-index", "--skip-worktree", "tracked.txt"])
        .unwrap();
    assert!(
        repo.git_without_test_sync_for_test(
            &[
                "checkout",
                &base,
                "--",
                &absolute_path(&repo, "tracked.txt")
            ],
            &[]
        )
        .is_err()
    );
    repo.sync_daemon_force();
    repo.git_og(&["update-index", "--no-skip-worktree", "tracked.txt"])
        .unwrap();
    repo.stage_all_and_commit("after skipped checkout").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

fn check_invocation(mode: &str, linked: bool) {
    let repo = if linked {
        TestRepo::new_worktree()
    } else {
        TestRepo::new()
    };
    let base = prepare_discard(&repo, "mock_ai");
    let subdir = repo.path().join("nested");
    fs::create_dir(&subdir).unwrap();
    let source = base.clone();
    let path = absolute_path(&repo, "tracked.txt");
    let args = ["checkout", &source, "--", &path];
    match mode {
        "root" => repo.git_without_test_sync_from_working_dir_for_test(repo.path(), &args, &[]),
        "subdir" => repo.git_without_test_sync_from_working_dir_for_test(&subdir, &args, &[]),
        "-C" => repo.git_without_test_sync_for_test(&args, &[]),
        _ => unreachable!(),
    }
    .unwrap();
    repo.sync_daemon_force();
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    repo.stage_all_and_commit("uncheckpointed recreation")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
}

#[test]
fn checkout_discard_invocation_from_root() {
    check_invocation("root", false);
}
#[test]
fn checkout_discard_invocation_from_subdir() {
    check_invocation("subdir", false);
}
#[test]
fn checkout_discard_invocation_with_c_flag() {
    check_invocation("-C", false);
}
#[test]
fn checkout_discard_invocation_from_root_in_worktree() {
    check_invocation("root", true);
}
#[test]
fn checkout_discard_invocation_from_subdir_in_worktree() {
    check_invocation("subdir", true);
}
#[test]
fn checkout_discard_invocation_with_c_flag_in_worktree() {
    check_invocation("-C", true);
}

fn check_backend(kind: git_ai::config::NotesBackendKind) {
    use git_ai::config::NotesBackendConfig;
    use git_ai::model::authorship_log_serialization::AuthorshipLog;
    use git_ai::model::repository::notes_db::NotesDatabase;
    use git_ai::notes::reference_server::ReferenceServer;

    let server = ReferenceServer::start("127.0.0.1:0").unwrap();
    let repo = TestRepo::new_with_daemon_env_and_patch(
        &[("GIT_AI_API_KEY", "checkout-backend-test-key")],
        |patch| {
            patch.notes_backend = Some(NotesBackendConfig {
                kind,
                backend_url: Some(server.base_url()),
            });
        },
    );
    let base = prepare_discard(&repo, "mock_ai");
    repo.git_without_test_sync_for_test(
        &[
            "checkout",
            &base,
            "--",
            &absolute_path(&repo, "tracked.txt"),
        ],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    let head = commit_all(&repo, "uncheckpointed recreation");
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
    let db = NotesDatabase::open_at_path(&repo.test_home_path().join(".git-ai/internal/notes-db"))
        .unwrap();
    let note = db
        .get_note(&head)
        .unwrap()
        .expect("backend must persist the commit note");
    let log = AuthorshipLog::deserialize_from_string(&note).unwrap();
    assert!(
        log.attestations
            .iter()
            .all(|file| file.file_path != "tracked.txt" || file.entries.is_empty())
    );
    assert!(repo.read_authorship_note(&head).is_none());
}

#[test]
fn checkout_discard_sqlite_backend() {
    check_backend(git_ai::config::NotesBackendKind::Sqlite);
}
#[test]
fn checkout_discard_http_backend() {
    check_backend(git_ai::config::NotesBackendKind::Http);
}

#[test]
fn checkout_discard_uses_own_worktree_head_after_another_worktree_commit() {
    let repo = TestRepo::new();
    let base = prepare_discard(&repo, "mock_ai");
    let other_dir = tempfile::tempdir().unwrap();
    let other = other_dir.path().join("linked");
    repo.git(&[
        "worktree",
        "add",
        "-b",
        "other",
        other.to_str().unwrap(),
        &base,
    ])
    .unwrap();
    repo.git_without_test_sync_from_working_dir_for_test(
        &other,
        &["commit", "--allow-empty", "-m", "other worktree head"],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    let blame = repo
        .git_ai_from_working_dir(&other, &["blame", "tracked.txt"])
        .unwrap();
    repos::test_file::TestFile::assert_committed_blame_output(&blame, lines!["base".human()]);
    assert_ne!(
        repo.git_og(&["-C", other.to_str().unwrap(), "rev-parse", "HEAD"])
            .unwrap()
            .trim(),
        base
    );
    repo.git_without_test_sync_for_test(
        &[
            "checkout",
            &base,
            "--",
            &absolute_path(&repo, "tracked.txt"),
        ],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    repo.stage_all_and_commit("own worktree recreation")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
    let blame = repo
        .git_ai_from_working_dir(&other, &["blame", "tracked.txt"])
        .unwrap();
    repos::test_file::TestFile::assert_committed_blame_output(&blame, lines!["base".human()]);
}

#[test]
fn checkout_discard_git_process_count_does_not_grow_with_pending_files() {
    let temp = tempfile::tempdir().unwrap();
    let mut counts = Vec::new();
    for count in [1, 8] {
        let log = temp.path().join(format!("spawns-{count}.log"));
        let repo = TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())]);
        let base = prepare_discard(&repo, "mock_ai");
        let others: Vec<_> = (0..count).map(|i| format!("pending-{i}.txt")).collect();
        for path in &others {
            fs::write(repo.path().join(path), "other ai\n").unwrap();
        }
        repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
        fs::write(&log, "").unwrap();
        repo.git_without_test_sync_for_test(
            &[
                "checkout",
                &base,
                "--",
                &absolute_path(&repo, "tracked.txt"),
            ],
            &[],
        )
        .unwrap();
        repo.sync_daemon_force();
        counts.push(fs::read_to_string(&log).unwrap().lines().count());
        fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
        repo.stage_all_and_commit("recreation and retained files")
            .unwrap();
        repo.filename("tracked.txt")
            .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
        for path in &others {
            repo.filename(path)
                .assert_committed_lines(lines!["other ai".ai()]);
        }
    }
    assert_eq!(
        counts[0], counts[1],
        "process count must be independent of pending-file count"
    );
    println!("checkout daemon Git process counts for 1 and 8 pending files: {counts:?}");
}

#[test]
fn checkout_discard_directory_and_config_override_keep_existing_boundary() {
    for config in [false, true] {
        let repo = TestRepo::new();
        let file = if config {
            "tracked.txt"
        } else {
            "directory/tracked.txt"
        };
        let base = prepare_discard_at(&repo, "mock_ai", file);
        let mut args = Vec::new();
        if config {
            args.extend(["-c", "core.quotePath=false"]);
        }
        args.extend(["checkout", &base, "--"]);
        let selected = absolute_path(&repo, if config { "tracked.txt" } else { "directory" });
        args.push(&selected);
        repo.git_without_test_sync_for_test(&args, &[]).unwrap();
        repo.sync_daemon_force();
        fs::write(repo.path().join(file), "discarded edit\n").unwrap();
        repo.stage_all_and_commit("existing unsupported boundary")
            .unwrap();
        repo.filename(file)
            .assert_committed_lines(lines!["discarded edit".ai()]);
    }
}

#[test]
fn checkout_discard_explicit_literal_mode_preserves_special_filename() {
    let repo = TestRepo::new();
    let file = "bracket[1].txt";
    let base = prepare_discard_at(&repo, "mock_ai", file);
    repo.git_without_test_sync_for_test(
        &[
            "--literal-pathspecs",
            "checkout",
            &base,
            "--",
            &absolute_path(&repo, "bracket[1].txt"),
        ],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    fs::write(repo.path().join(file), "discarded edit\n").unwrap();
    repo.stage_all_and_commit("literal path recreation")
        .unwrap();
    repo.filename(file)
        .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
}

#[test]
fn checkout_discard_plain_path_with_proven_absolute_root() {
    let repo = TestRepo::new();
    let base = prepare_discard(&repo, "mock_ai");
    let root = repo.git_og(&["rev-parse", "--show-toplevel"]).unwrap();
    repo.git_without_test_sync_for_test(
        &["-C", root.trim(), "checkout", &base, "--", "tracked.txt"],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    repo.stage_all_and_commit("proven root recreation").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".unattributed_human()]);
}

#[test]
#[cfg(unix)]
fn checkout_discard_literal_environment_does_not_clear_another_file() {
    let repo = TestRepo::new();
    let shadow = ":(top,literal)tracked.txt";
    for path in ["tracked.txt", shadow] {
        fs::write(repo.path().join(path), "base\n").unwrap();
        repo.git_ai(&["checkpoint", "mock_known_human", path])
            .unwrap();
    }
    let base = commit_all(&repo, "two distinct filenames");
    for path in ["tracked.txt", shadow] {
        repo.filename(path)
            .assert_committed_lines(lines!["base".human()]);
    }
    fs::write(repo.path().join("tracked.txt"), "retained ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "tracked.txt"])
        .unwrap();
    fs::write(repo.path().join(shadow), "discarded edit\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", shadow]).unwrap();
    repo.git_without_test_sync_for_test(
        &["checkout", &base, "--", shadow],
        &[("GIT_LITERAL_PATHSPECS", "1")],
    )
    .unwrap();
    repo.sync_daemon_force();
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "retained ai\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join(shadow)).unwrap(),
        "base\n"
    );
    repo.stage_all_and_commit("retained edit after literal-path checkout")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["retained ai".ai()]);
    repo.filename(shadow)
        .assert_committed_lines(lines!["base".human()]);
}
