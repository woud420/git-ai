#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;
use std::process::Stdio;
use std::time::{Duration, Instant};

fn write_checkpoint(repo: &TestRepo, path: &str, contents: &str, preset: &str) {
    fs::write(repo.path().join(path), contents).unwrap();
    repo.git_ai(&["checkpoint", preset, path]).unwrap();
}

fn assert_base(repo: &TestRepo) {
    repo.filename("base.txt")
        .assert_committed_lines(lines!["base ai".ai()]);
}

fn assert_feature(repo: &TestRepo) {
    assert_base(repo);
    repo.filename("feature.txt")
        .assert_committed_lines(lines!["feature ai".ai()]);
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

fn prepare_feature(repo: &TestRepo, tracked: bool) -> (String, String) {
    write_checkpoint(repo, "base.txt", "base ai\n", "mock_ai");
    if tracked {
        write_checkpoint(repo, "local.txt", "local base\n", "mock_known_human");
    }
    let base = commit_all(repo, "base");
    assert_base(repo);
    if tracked {
        repo.filename("local.txt")
            .assert_committed_lines(lines!["local base".human()]);
    }
    let branch = repo.current_branch();
    repo.git(&["switch", "-c", "feature"]).unwrap();
    write_checkpoint(repo, "feature.txt", "feature ai\n", "mock_ai");
    let feature = commit_all(repo, "feature");
    assert_feature(repo);
    if tracked {
        repo.filename("local.txt")
            .assert_committed_lines(lines!["local base".human()]);
    }
    repo.git(&["switch", &branch]).unwrap();
    (base, feature)
}

fn run_merge(repo: &TestRepo, args: &[&str]) -> Result<String, String> {
    // Match production argv: TestRepo::git adds a -c correlation marker, while
    // this support profile deliberately excludes global config overrides.
    let result = repo.git_without_test_sync_for_test(args, &[]);
    repo.sync_daemon_force();
    result
}

fn assert_pending_commit(repo: &TestRepo) {
    let commit = repo.stage_all_and_commit("local after merge").unwrap();
    assert_feature(repo);
    repo.filename("local.txt")
        .assert_committed_lines(lines!["local ai".ai()]);
    assert!(
        commit
            .authorship_log
            .attestations
            .iter()
            .any(|file| { file.file_path == "local.txt" && !file.entries.is_empty() }),
        "surviving edit must have a persisted attestation"
    );
}

#[test]
fn merge_carryover_untracked_file() {
    let repo = TestRepo::new();
    let (_, feature) = prepare_feature(&repo, false);
    write_checkpoint(&repo, "local.txt", "local ai\n", "mock_ai");
    run_merge(&repo, &["merge", "--ff-only", "feature"]).unwrap();
    assert_eq!(repo.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), feature);
    assert_pending_commit(&repo);
}

#[test]
fn merge_carryover_tracked_file() {
    let repo = TestRepo::new();
    prepare_feature(&repo, true);
    write_checkpoint(&repo, "local.txt", "local ai\n", "mock_ai");
    run_merge(&repo, &["merge", "--ff-only", "feature"]).unwrap();
    assert_pending_commit(&repo);
}

#[test]
fn merge_carryover_staged_file_and_trailing_flag() {
    let repo = TestRepo::new();
    prepare_feature(&repo, true);
    write_checkpoint(&repo, "local.txt", "local ai\n", "mock_ai");
    repo.git(&["add", "local.txt"]).unwrap();
    run_merge(&repo, &["merge", "feature", "--ff-only"]).unwrap();
    assert_pending_commit(&repo);
}

#[test]
fn merge_carryover_known_human_attestation() {
    let repo = TestRepo::new();
    prepare_feature(&repo, true);
    write_checkpoint(&repo, "local.txt", "known human\n", "mock_known_human");
    run_merge(&repo, &["merge", "--ff-only", "feature"]).unwrap();
    let commit = repo.stage_all_and_commit("local known human").unwrap();
    assert_feature(&repo);
    repo.filename("local.txt")
        .assert_committed_lines(lines!["known human".human()]);
    assert!(commit.authorship_log.attestations.iter().any(|file| {
        file.file_path == "local.txt"
            && file
                .entries
                .iter()
                .any(|entry| entry.hash.starts_with("h_"))
    }));
}

#[test]
fn merge_carryover_noop_and_failed_commands_preserve_evidence() {
    for target in ["HEAD", "missing-ref"] {
        let repo = TestRepo::new();
        let (base, _) = prepare_feature(&repo, true);
        write_checkpoint(&repo, "local.txt", "local ai\n", "mock_ai");
        let result = run_merge(&repo, &["merge", "--ff-only", target]);
        assert_eq!(result.is_ok(), target == "HEAD");
        assert_eq!(repo.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), base);
        repo.stage_all_and_commit("after unchanged head").unwrap();
        assert_base(&repo);
        repo.filename("local.txt")
            .assert_committed_lines(lines!["local ai".ai()]);
    }
}

#[test]
fn merge_carryover_failed_divergent_fast_forward() {
    let repo = TestRepo::new();
    prepare_feature(&repo, false);
    write_checkpoint(&repo, "diverged.txt", "diverged ai\n", "mock_ai");
    let before = repo.stage_all_and_commit("diverged").unwrap().commit_sha;
    assert_base(&repo);
    repo.filename("diverged.txt")
        .assert_committed_lines(lines!["diverged ai".ai()]);
    write_checkpoint(&repo, "local.txt", "local ai\n", "mock_ai");
    assert!(run_merge(&repo, &["merge", "--ff-only", "feature"]).is_err());
    assert_eq!(repo.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), before);
    repo.stage_all_and_commit("after rejected merge").unwrap();
    assert_base(&repo);
    repo.filename("diverged.txt")
        .assert_committed_lines(lines!["diverged ai".ai()]);
    repo.filename("local.txt")
        .assert_committed_lines(lines!["local ai".ai()]);
}

#[test]
fn merge_carryover_delayed_past_another_merge_and_checkpoint() {
    let temp = tempfile::tempdir().unwrap();
    let gate = temp.path().join("merge-gate");
    fs::write(&gate, "hold").unwrap();
    let spec = format!("merge={}", gate.display());
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND", &spec)]);
    let (base, feature) = prepare_feature(&repo, false);
    let branch = repo.current_branch();
    repo.git(&["switch", "-c", "feature-two", "feature"])
        .unwrap();
    write_checkpoint(&repo, "second.txt", "second ai\n", "mock_ai");
    let second = repo
        .stage_all_and_commit("second feature")
        .unwrap()
        .commit_sha;
    assert_feature(&repo);
    repo.filename("second.txt")
        .assert_committed_lines(lines!["second ai".ai()]);
    repo.git(&["switch", &branch]).unwrap();
    write_checkpoint(&repo, "local.txt", "local ai\n", "mock_ai");
    repo.git_without_test_sync_for_test(&["merge", "--ff-only", &feature], &[])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            Instant::now() < deadline,
            "merge did not reach side-effect gate"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    repo.git_without_test_sync_for_test(&["merge", "--ff-only", &second], &[])
        .unwrap();
    assert_ne!(base, second);
    assert_eq!(repo.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), second);
    fs::write(repo.path().join("later.txt"), "later human\n").unwrap();
    let child = repo
        .git_ai_command_without_pre_sync_for_test(
            &["checkpoint", "mock_known_human", "later.txt"],
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
    assert_pending_commit(&repo);
    repo.filename("second.txt")
        .assert_committed_lines(lines!["second ai".ai()]);
    repo.filename("later.txt")
        .assert_committed_lines(lines!["later human".human()]);
}

#[test]
fn merge_carryover_git_process_count_does_not_grow_with_files() {
    let temp = tempfile::tempdir().unwrap();
    let mut counts = Vec::new();
    for file_count in [1, 8] {
        let log_path = temp.path().join(format!("spawns-{file_count}.log"));
        let log_path_string = log_path.to_string_lossy().to_string();
        let repo = TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", &log_path_string)]);
        prepare_feature(&repo, false);
        let files: Vec<_> = (0..file_count)
            .map(|i| format!("pending-{i}.txt"))
            .collect();
        for file in &files {
            fs::write(repo.path().join(file), "pending ai\n").unwrap();
        }
        repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
        fs::write(&log_path, "").unwrap();
        run_merge(&repo, &["merge", "--ff-only", "feature"]).unwrap();
        repo.sync_daemon_force();
        let commands = fs::read_to_string(&log_path).unwrap();
        counts.push(commands.lines().count());
        repo.stage_all_and_commit("pending files").unwrap();
        assert_feature(&repo);
        for file in &files {
            repo.filename(file)
                .assert_committed_lines(lines!["pending ai".ai()]);
        }
    }
    assert_eq!(
        counts[0], counts[1],
        "merge Git process count must be independent of file count"
    );
    println!("merge daemon Git process counts for 1 and 8 files: {counts:?}");
}

fn check_invocation(mode: &str, worktree: bool) {
    let repo = if worktree {
        TestRepo::new_worktree()
    } else {
        TestRepo::new()
    };
    prepare_feature(&repo, true);
    write_checkpoint(&repo, "local.txt", "local ai\n", "mock_ai");
    let subdir = repo.path().join("nested");
    fs::create_dir(&subdir).unwrap();
    let args = ["merge", "--ff-only", "feature"];
    match mode {
        "root" => repo.git_without_test_sync_from_working_dir_for_test(repo.path(), &args, &[]),
        "subdir" => repo.git_without_test_sync_from_working_dir_for_test(&subdir, &args, &[]),
        "-C" => repo.git_without_test_sync_for_test(&args, &[]),
        _ => unreachable!(),
    }
    .unwrap();
    repo.sync_daemon_force();
    assert_pending_commit(&repo);
}

#[test]
fn merge_carryover_invocation_from_root() {
    check_invocation("root", false);
}
#[test]
fn merge_carryover_invocation_from_subdir() {
    check_invocation("subdir", false);
}
#[test]
fn merge_carryover_invocation_with_c_flag() {
    check_invocation("-C", false);
}
#[test]
fn merge_carryover_invocation_from_root_in_worktree() {
    check_invocation("root", true);
}
#[test]
fn merge_carryover_invocation_from_subdir_in_worktree() {
    check_invocation("subdir", true);
}
#[test]
fn merge_carryover_invocation_with_c_flag_in_worktree() {
    check_invocation("-C", true);
}

fn check_backend(kind: git_ai::config::NotesBackendKind) {
    use git_ai::config::NotesBackendConfig;
    use git_ai::model::authorship_log_serialization::AuthorshipLog;
    use git_ai::model::repository::notes_db::NotesDatabase;
    use git_ai::notes::reference_server::ReferenceServer;

    let server = ReferenceServer::start("127.0.0.1:0").unwrap();
    let repo = TestRepo::new_with_daemon_env_and_patch(
        &[("GIT_AI_API_KEY", "merge-backend-test-key")],
        |patch| {
            patch.notes_backend = Some(NotesBackendConfig {
                kind,
                backend_url: Some(server.base_url()),
            });
        },
    );
    prepare_feature(&repo, false);
    write_checkpoint(&repo, "local.txt", "local ai\n", "mock_ai");
    run_merge(&repo, &["merge", "--ff-only", "feature"]).unwrap();
    let head = commit_all(&repo, "pending backend attribution");
    assert_feature(&repo);
    repo.filename("local.txt")
        .assert_committed_lines(lines!["local ai".ai()]);
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
            .any(|file| file.file_path == "local.txt" && !file.entries.is_empty())
    );
    assert!(repo.read_authorship_note(&head).is_none());
}

#[test]
fn merge_carryover_sqlite_backend() {
    check_backend(git_ai::config::NotesBackendKind::Sqlite);
}

#[test]
fn merge_carryover_http_backend() {
    check_backend(git_ai::config::NotesBackendKind::Http);
}

#[test]
fn merge_carryover_config_override_keeps_existing_boundary() {
    let repo = TestRepo::new();
    let (base, feature) = prepare_feature(&repo, false);
    write_checkpoint(&repo, "local.txt", "local ai\n", "mock_ai");
    run_merge(
        &repo,
        &["-c", "merge.ff=only", "merge", "--ff-only", "feature"],
    )
    .unwrap();
    assert_eq!(repo.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), feature);
    let repository =
        git_ai::operations::git::find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    assert!(repository.storage.has_working_log(&base));
    assert!(!repository.storage.has_working_log(&feature));
    assert_feature(&repo);
    assert_eq!(
        fs::read_to_string(repo.path().join("local.txt")).unwrap(),
        "local ai\n"
    );
}

#[test]
fn merge_carryover_preserves_merge_commit_and_no_commit_modes() {
    for no_commit in [false, true] {
        let repo = TestRepo::new();
        let (base, _) = prepare_feature(&repo, false);
        if no_commit {
            run_merge(&repo, &["merge", "--no-ff", "--no-commit", "feature"]).unwrap();
            assert_eq!(repo.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), base);
            commit_all(&repo, "merge after no-commit");
        } else {
            run_merge(
                &repo,
                &["merge", "--no-ff", "-m", "merge feature", "feature"],
            )
            .unwrap();
        }
        assert_feature(&repo);
        assert_eq!(
            repo.git_og(&["rev-list", "--parents", "-n", "1", "HEAD"])
                .unwrap()
                .split_whitespace()
                .count(),
            3
        );
    }
}

#[test]
fn merge_carryover_preserves_conflict_abort_and_pending_attribution() {
    let repo = TestRepo::new();
    write_checkpoint(&repo, "conflict.txt", "base\n", "mock_known_human");
    commit_all(&repo, "base");
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["base".human()]);
    let branch = repo.current_branch();
    repo.git(&["switch", "-c", "feature"]).unwrap();
    write_checkpoint(&repo, "conflict.txt", "feature ai\n", "mock_ai");
    commit_all(&repo, "feature conflict");
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["feature ai".ai()]);
    repo.git(&["switch", &branch]).unwrap();
    write_checkpoint(&repo, "conflict.txt", "main ai\n", "mock_ai");
    let before = commit_all(&repo, "main conflict");
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["main ai".ai()]);
    write_checkpoint(&repo, "local.txt", "local ai\n", "mock_ai");
    assert!(run_merge(&repo, &["merge", "feature"]).is_err());
    assert!(
        fs::read_to_string(repo.path().join("conflict.txt"))
            .unwrap()
            .contains("<<<<<<<")
    );
    run_merge(&repo, &["merge", "--abort"]).unwrap();
    assert_eq!(repo.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), before);
    commit_all(&repo, "pending after abort");
    repo.filename("conflict.txt")
        .assert_committed_lines(lines!["main ai".ai()]);
    repo.filename("local.txt")
        .assert_committed_lines(lines!["local ai".ai()]);
}
