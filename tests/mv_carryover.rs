#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;
use std::process::Stdio;
use std::time::{Duration, Instant};

fn commit_all(repo: &TestRepo, message: &str) -> String {
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "-m", message]).unwrap();
    repo.sync_daemon_force();
    repo.git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

fn prepare(repo: &TestRepo, source: &str, preset: &str) {
    let path = repo.path().join(source);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", source])
        .unwrap();
    commit_all(repo, "base");
    repo.filename(source)
        .assert_committed_lines(lines!["base".human()]);
    fs::write(&path, "pending edit\n").unwrap();
    repo.git_ai(&["checkpoint", preset, source]).unwrap();
}

fn move_to_root(repo: &TestRepo, source: &str) {
    rooted_move(repo, &["--", source, "."], &[]).unwrap();
    repo.sync_daemon_force();
}

fn rooted_move(repo: &TestRepo, args: &[&str], env: &[(&str, &str)]) -> Result<String, String> {
    let root = repo.git_og(&["rev-parse", "--show-toplevel"]).unwrap();
    let mut command = vec!["-C", root.trim(), "mv"];
    command.extend_from_slice(args);
    repo.git_without_test_sync_for_test(&command, env)
}

#[test]
fn mv_carryover_ai_edit_before_move() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    move_to_root(&repo, "nested/file.txt");
    repo.stage_all_and_commit("move pending AI edit").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
}

#[test]
fn mv_carryover_keeps_committed_ai_beside_pending_ai() {
    let repo = TestRepo::new();
    fs::create_dir_all(repo.path().join("nested")).unwrap();
    let source = "nested/file.txt";
    fs::write(repo.path().join(source), "committed AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", source]).unwrap();
    commit_all(&repo, "committed AI");
    repo.filename(source)
        .assert_committed_lines(lines!["committed AI".ai()]);
    fs::write(repo.path().join(source), "committed AI\npending AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", source]).unwrap();
    move_to_root(&repo, source);
    repo.stage_all_and_commit("move committed and pending AI")
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["committed AI".ai(), "pending AI".ai()]);
}

#[test]
fn mv_carryover_known_human_edit_before_move() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_known_human");
    move_to_root(&repo, "nested/file.txt");
    let commit = repo
        .stage_all_and_commit("move pending known-human edit")
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".human()]);
    assert!(
        commit.authorship_log.attestations.iter().any(|file| {
            file.file_path == "file.txt"
                && file.entries.iter().any(|entry| {
                    entry.hash.starts_with("h_")
                        && commit
                            .authorship_log
                            .metadata
                            .humans
                            .contains_key(&entry.hash)
                })
        }),
        "known human input must retain its attestation, not merely a Git author"
    );
}

#[test]
fn mv_carryover_directory_preserves_pending_descendant() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/directory/file.txt", "mock_ai");
    move_to_root(&repo, "nested/directory");
    repo.stage_all_and_commit("move directory").unwrap();
    repo.filename("directory/file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
}

#[test]
fn mv_carryover_later_old_path_recreation_is_untracked() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    move_to_root(&repo, "nested/file.txt");
    fs::write(repo.path().join("nested/file.txt"), "pending edit\n").unwrap();
    repo.stage_all_and_commit("recreate old name after move")
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
    repo.filename("nested/file.txt")
        .assert_committed_lines(lines!["pending edit".unattributed_human()]);
}

#[test]
fn mv_carryover_checkpoint_after_move_keeps_prior_ai() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    move_to_root(&repo, "nested/file.txt");
    fs::write(repo.path().join("file.txt"), "pending edit\nhuman edit\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "file.txt"])
        .unwrap();
    repo.stage_all_and_commit("edit after move").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".ai(), "human edit".human()]);
}

#[test]
fn mv_carryover_dry_run_preserves_source() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    rooted_move(&repo, &["--dry-run", "--", "nested/file.txt", "."], &[]).unwrap();
    repo.sync_daemon_force();
    repo.stage_all_and_commit("dry run does not move evidence")
        .unwrap();
    repo.filename("nested/file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
    assert!(!repo.path().join("file.txt").exists());
}

#[test]
fn mv_carryover_failed_move_preserves_source() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    fs::write(repo.path().join("file.txt"), "existing destination\n").unwrap();
    assert!(rooted_move(&repo, &["--", "nested/file.txt", "."], &[]).is_err());
    repo.sync_daemon_force();
    repo.stage_all_and_commit("failed move keeps source evidence")
        .unwrap();
    repo.filename("nested/file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
    repo.filename("file.txt")
        .assert_committed_lines(lines!["existing destination".unattributed_human()]);
}

#[test]
fn mv_carryover_unrelated_pending_file_is_preserved() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    fs::write(repo.path().join("other.txt"), "other AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "other.txt"])
        .unwrap();
    move_to_root(&repo, "nested/file.txt");
    repo.stage_all_and_commit("move one pending file").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
    repo.filename("other.txt")
        .assert_committed_lines(lines!["other AI".ai()]);
}

#[test]
fn mv_carryover_preserves_later_checkpoint_during_delayed_processing() {
    let temp = tempfile::tempdir().unwrap();
    let gate = temp.path().join("mv-gate");
    fs::write(&gate, "hold").unwrap();
    let spec = format!("mv={}", gate.display());
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND", &spec)]);
    prepare(&repo, "nested/file.txt", "mock_ai");
    rooted_move(&repo, &["--", "nested/file.txt", "."], &[]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            Instant::now() < deadline,
            "move never reached its side-effect gate"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(repo.path().join("file.txt"), "pending edit\nlater human\n").unwrap();
    let child = repo
        .git_ai_command_without_pre_sync_for_test(
            &["checkpoint", "mock_known_human", "file.txt"],
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
    repo.stage_all_and_commit("later checkpoint keeps moved evidence")
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".ai(), "later human".human()]);
}

#[test]
fn mv_carryover_alternate_index_keeps_default_staged_evidence() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    repo.git(&["add", "nested/file.txt"]).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let alternate = temp.path().join("index");
    let env = [("GIT_INDEX_FILE", alternate.to_str().unwrap())];
    repo.git_og_with_env(&["read-tree", "HEAD"], &env).unwrap();
    rooted_move(&repo, &["--", "nested/file.txt", "."], &env).unwrap();
    repo.sync_daemon_force();
    fs::write(repo.path().join("nested/file.txt"), "pending edit\n").unwrap();
    repo.commit("default staged path survives alternate-index move")
        .unwrap();
    repo.filename("nested/file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
    assert!(repo.git_og(&["show", "HEAD:file.txt"]).is_err());
}

#[test]
fn mv_carryover_stale_destination_does_not_override_moved_ai() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    fs::write(repo.path().join("file.txt"), "pending edit\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "file.txt"])
        .unwrap();
    fs::remove_file(repo.path().join("file.txt")).unwrap();
    move_to_root(&repo, "nested/file.txt");
    repo.stage_all_and_commit("replace stale destination evidence")
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
}

#[test]
fn mv_carryover_untracked_copy_does_not_inherit_moved_attribution() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    fs::copy(
        repo.path().join("nested/file.txt"),
        repo.path().join("copy.txt"),
    )
    .unwrap();
    move_to_root(&repo, "nested/file.txt");
    repo.stage_all_and_commit("move with an equal-content copy")
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
    repo.filename("copy.txt")
        .assert_committed_lines(lines!["pending edit".unattributed_human()]);
}

#[test]
fn mv_carryover_linked_worktree() {
    let repo = TestRepo::new_worktree();
    prepare(&repo, "nested/file.txt", "mock_ai");
    move_to_root(&repo, "nested/file.txt");
    repo.stage_all_and_commit("move in linked worktree")
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
}

#[test]
fn mv_carryover_special_literal_filename() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/bracket[1].txt", "mock_ai");
    let root = repo.git_og(&["rev-parse", "--show-toplevel"]).unwrap();
    repo.git_without_test_sync_for_test(
        &[
            "-C",
            root.trim(),
            "--literal-pathspecs",
            "mv",
            "--",
            "nested/bracket[1].txt",
            ".",
        ],
        &[],
    )
    .unwrap();
    repo.sync_daemon_force();
    repo.stage_all_and_commit("move literal filename").unwrap();
    repo.filename("bracket[1].txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
}

#[test]
fn mv_carryover_sparse_skipped_path_retains_evidence() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    repo.git_og(&["update-index", "--skip-worktree", "nested/file.txt"])
        .unwrap();
    fs::remove_file(repo.path().join("nested/file.txt")).unwrap();
    assert!(rooted_move(&repo, &["--", "nested/file.txt", "."], &[]).is_err());
    repo.sync_daemon_force();
    repo.git_og(&["update-index", "--no-skip-worktree", "nested/file.txt"])
        .unwrap();
    fs::write(repo.path().join("nested/file.txt"), "pending edit\n").unwrap();
    repo.stage_all_and_commit("skipped path keeps evidence")
        .unwrap();
    repo.filename("nested/file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
}

#[test]
fn mv_carryover_unsupported_forms_leave_working_log_unchanged() {
    for form in [
        "force",
        "skip-errors",
        "named-destination",
        "missing-separator",
        "multiple-sources",
    ] {
        let repo = TestRepo::new();
        prepare(&repo, "nested/file.txt", "mock_ai");
        fs::write(repo.path().join("nested/other.txt"), "other\n").unwrap();
        repo.git(&["add", "nested/other.txt"]).unwrap();
        let before =
            serde_json::to_value(repo.current_working_logs().read_all_checkpoints().unwrap())
                .unwrap();
        let args = match form {
            "force" => vec!["--force", "--", "nested/file.txt", "."],
            "skip-errors" => vec!["-k", "--", "nested/file.txt", "."],
            "named-destination" => vec!["--", "nested/file.txt", "file.txt"],
            "missing-separator" => vec!["nested/file.txt", "."],
            "multiple-sources" => vec!["--", "nested/file.txt", "nested/other.txt", "."],
            _ => unreachable!(),
        };
        rooted_move(&repo, &args, &[]).unwrap();
        repo.sync_daemon_force();
        assert_eq!(
            serde_json::to_value(repo.current_working_logs().read_all_checkpoints().unwrap())
                .unwrap(),
            before,
            "{form}"
        );
        repo.stage_all_and_commit("unchanged unsupported move profile")
            .unwrap();
        repo.filename("file.txt")
            .assert_committed_lines(lines!["pending edit".unattributed_human()]);
        repo.filename(if form == "multiple-sources" {
            "other.txt"
        } else {
            "nested/other.txt"
        })
        .assert_committed_lines(lines!["other".unattributed_human()]);
    }
}

#[test]
fn mv_carryover_git_process_count_is_constant_for_directory_size() {
    let temp = tempfile::tempdir().unwrap();
    let mut counts = Vec::new();
    for count in [1, 8] {
        let log = temp.path().join(format!("spawns-{count}.log"));
        let repo = TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())]);
        fs::create_dir_all(repo.path().join("nested/directory")).unwrap();
        let paths: Vec<_> = (0..count)
            .map(|i| format!("nested/directory/file-{i}.txt"))
            .collect();
        for path in &paths {
            fs::write(repo.path().join(path), "base\n").unwrap();
        }
        repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();
        commit_all(&repo, "base");
        for path in &paths {
            repo.filename(path)
                .assert_committed_lines(lines!["base".human()]);
        }
        for path in &paths {
            fs::write(repo.path().join(path), "pending edit\n").unwrap();
        }
        repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
        fs::write(&log, "").unwrap();
        move_to_root(&repo, "nested/directory");
        counts.push(fs::read_to_string(&log).unwrap().lines().count());
        repo.stage_all_and_commit("move all pending descendants")
            .unwrap();
        for i in 0..count {
            repo.filename(&format!("directory/file-{i}.txt"))
                .assert_committed_lines(lines!["pending edit".ai()]);
        }
    }
    assert_eq!(counts[0], counts[1]);
    println!("mv daemon Git process counts for 1 and 8 moved files: {counts:?}");
}

fn check_backend(kind: git_ai::config::NotesBackendKind) {
    use git_ai::config::NotesBackendConfig;
    use git_ai::model::authorship_log_serialization::AuthorshipLog;
    use git_ai::model::repository::notes_db::NotesDatabase;
    use git_ai::notes::reference_server::ReferenceServer;
    let server = ReferenceServer::start("127.0.0.1:0").unwrap();
    let repo =
        TestRepo::new_with_daemon_env_and_patch(&[("GIT_AI_API_KEY", "mv-fixture-key")], |patch| {
            patch.notes_backend = Some(NotesBackendConfig {
                kind,
                backend_url: Some(server.base_url()),
            })
        });
    prepare(&repo, "nested/file.txt", "mock_ai");
    move_to_root(&repo, "nested/file.txt");
    let head = commit_all(&repo, "move pending backend evidence");
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".ai()]);
    let db = NotesDatabase::open_at_path(&repo.test_home_path().join(".git-ai/internal/notes-db"))
        .unwrap();
    let note = db.get_note(&head).unwrap().unwrap();
    let log = AuthorshipLog::deserialize_from_string(&note).unwrap();
    assert!(
        log.attestations
            .iter()
            .any(|file| file.file_path == "file.txt" && !file.entries.is_empty())
    );
    assert!(repo.read_authorship_note(&head).is_none());
}

#[test]
fn mv_carryover_sqlite_backend() {
    check_backend(git_ai::config::NotesBackendKind::Sqlite);
}

#[test]
fn mv_carryover_http_backend() {
    check_backend(git_ai::config::NotesBackendKind::Http);
}

#[test]
fn mv_carryover_initial_state_keeps_existing_boundary() {
    let repo = TestRepo::new();
    prepare(&repo, "nested/file.txt", "mock_ai");
    let log = repo.current_working_logs();
    let checkpoints = log.read_all_checkpoints().unwrap();
    let entry = checkpoints
        .iter()
        .flat_map(|checkpoint| &checkpoint.entries)
        .find(|entry| entry.file == "nested/file.txt")
        .unwrap();
    let mut initial = git_ai::model::working_log::InitialAttributions::default();
    initial
        .files
        .insert(entry.file.clone(), entry.line_attributions.clone());
    initial
        .file_blobs
        .insert(entry.file.clone(), entry.blob_sha.clone());
    log.write_initial(initial).unwrap();
    let before = fs::read(log.checkpoints_file()).unwrap();
    let initial_before = fs::read(&log.initial_file).unwrap();
    move_to_root(&repo, "nested/file.txt");
    assert_eq!(fs::read(log.checkpoints_file()).unwrap(), before);
    assert_eq!(fs::read(&log.initial_file).unwrap(), initial_before);
    repo.stage_all_and_commit("unsupported INITIAL carryover")
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["pending edit".unattributed_human()]);
}

#[test]
fn mv_carryover_collection_opt_out_preserves_existing_journal() {
    let mut repo = TestRepo::new_dedicated_daemon();
    prepare(&repo, "nested/file.txt", "mock_ai");
    let log = repo.current_working_logs();
    let before = fs::read(log.checkpoints_file()).unwrap();
    repo.patch_git_ai_config(|patch| {
        patch.allowed_repositories = Some(Vec::new());
    });
    move_to_root(&repo, "nested/file.txt");
    assert_eq!(fs::read(log.checkpoints_file()).unwrap(), before);
    assert_eq!(
        fs::read_to_string(repo.path().join("file.txt")).unwrap(),
        "pending edit\n"
    );
}
