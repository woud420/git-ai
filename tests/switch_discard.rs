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

fn switch(repo: &TestRepo, args: &[&str]) {
    let mut command = vec!["--no-replace-objects", "switch"];
    command.extend_from_slice(args);
    repo.git_without_test_sync_for_test(&command, &[]).unwrap();
    repo.sync_daemon_force();
}

fn branch(repo: &TestRepo) -> String {
    repo.git_og(&["branch", "--show-current"])
        .unwrap()
        .trim()
        .to_owned()
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
            .all(|file| { file.file_path != "tracked.txt" || file.entries.is_empty() })
    );
}

#[test]
fn switch_discard_same_branch_ai() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    switch(&repo, &["--discard-changes", &branch(&repo)]);
    assert_untracked_recreation(&repo);
}

#[test]
fn switch_discard_same_branch_force_known_human() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_known_human");
    switch(&repo, &["--force", &branch(&repo)]);
    assert_untracked_recreation(&repo);
}

#[test]
fn switch_discard_same_oid_branch_identity() {
    let repo = TestRepo::new();
    let head = commit_base(&repo);
    repo.git_og(&["branch", "other", &head]).unwrap();
    checkpoint_replacement(&repo, "mock_ai");
    switch(&repo, &["-f", "other"]);
    assert_eq!(branch(&repo), "other");
    assert_eq!(repo.git_og(&["rev-parse", "HEAD"]).unwrap().trim(), head);
    assert_untracked_recreation(&repo);
    assert_eq!(branch(&repo), "other");
}

#[test]
fn switch_discard_same_oid_detach() {
    let repo = TestRepo::new();
    let head = commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    switch(&repo, &["--discard-changes", "--detach", &head]);
    assert!(branch(&repo).is_empty());
    assert_untracked_recreation(&repo);
}

#[test]
fn switch_discard_from_subdirectory_and_c() {
    for via_c in [false, true] {
        let repo = TestRepo::new();
        commit_base(&repo);
        checkpoint_replacement(&repo, "mock_ai");
        let subdir = repo.path().join("nested");
        fs::create_dir(&subdir).unwrap();
        let branch = branch(&repo);
        let mut args = vec!["--no-replace-objects"];
        if via_c {
            args.extend(["-C", subdir.to_str().unwrap()]);
        }
        args.extend(["switch", "--discard-changes", &branch]);
        repo.git_without_test_sync_from_working_dir_for_test(
            if via_c { repo.path() } else { &subdir },
            &args,
            &[],
        )
        .unwrap();
        repo.sync_daemon_force();
        assert_untracked_recreation(&repo);
    }
}

#[test]
fn switch_discard_preserves_untracked_file_evidence() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    fs::write(repo.path().join("untracked.txt"), "surviving ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "untracked.txt"])
        .unwrap();
    switch(&repo, &["--discard-changes", &branch(&repo)]);
    assert_untracked_recreation(&repo);
    repo.filename("untracked.txt")
        .assert_committed_lines(lines!["surviving ai".ai()]);
}

#[test]
fn switch_discard_alternate_index_preserves_default_staged_evidence() {
    let repo = TestRepo::new();
    let head = commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git(&["add", "tracked.txt"]).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let index = temp.path().join("alternate-index");
    let env = [("GIT_INDEX_FILE", index.to_str().unwrap())];
    repo.git_og_with_env(&["read-tree", &head], &env).unwrap();
    repo.git_without_test_sync_for_test(
        &[
            "--no-replace-objects",
            "switch",
            "--discard-changes",
            &branch(&repo),
        ],
        &env,
    )
    .unwrap();
    repo.sync_daemon_force();
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
    repo.commit("default staged edit survives").unwrap();
    assert_eq!(
        repo.git_og(&["show", "HEAD:tracked.txt"]).unwrap(),
        "discarded edit\n"
    );
    fs::write(repo.path().join("tracked.txt"), "discarded edit\n").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn switch_discard_skip_worktree_preserves_retained_edit() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git_og(&["update-index", "--skip-worktree", "tracked.txt"])
        .unwrap();
    switch(&repo, &["--discard-changes", &branch(&repo)]);
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
fn switch_discard_assume_unchanged_matches_native_file_retention() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git_og(&["update-index", "--assume-unchanged", "tracked.txt"])
        .unwrap();
    switch(&repo, &["--discard-changes", &branch(&repo)]);
    let retained = fs::read_to_string(repo.path().join("tracked.txt")).unwrap();
    repo.git_og(&["update-index", "--no-assume-unchanged", "tracked.txt"])
        .unwrap();
    match retained.as_str() {
        "base\n" => assert_untracked_recreation(&repo),
        "discarded edit\n" => {
            repo.stage_all_and_commit("retained assume-unchanged edit")
                .unwrap();
            repo.filename("tracked.txt")
                .assert_committed_lines(lines!["discarded edit".ai()]);
        }
        _ => panic!("unexpected native switch result: {retained:?}"),
    }
}

#[test]
fn switch_discard_collection_opt_out_preserves_existing_journal() {
    let mut repo = TestRepo::new_dedicated_daemon();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    let log = repo.current_working_logs();
    let before = fs::read(log.checkpoints_file()).unwrap();
    repo.patch_git_ai_config(|patch| patch.allowed_repositories = Some(Vec::new()));
    switch(&repo, &["--discard-changes", &branch(&repo)]);
    assert!(fs::read(log.checkpoints_file()).unwrap() == before);
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
}

#[test]
fn switch_discard_normal_same_branch_preserves_evidence() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    switch(&repo, &[&branch(&repo)]);
    repo.stage_all_and_commit("preserved local edit").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn switch_discard_failure_preserves_evidence() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    assert!(
        repo.git(&[
            "--no-replace-objects",
            "switch",
            "--discard-changes",
            "missing-branch"
        ])
        .is_err()
    );
    repo.stage_all_and_commit("edit after failed switch")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
}

#[test]
fn switch_discard_preserves_checkpoint_after_unsynchronized_switch() {
    let temp = tempfile::tempdir().unwrap();
    let gate = temp.path().join("switch-gate");
    fs::write(&gate, "hold").unwrap();
    let spec = format!("switch={}", gate.display());
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND", &spec)]);
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git_without_test_sync_for_test(
        &[
            "--no-replace-objects",
            "switch",
            "--discard-changes",
            &branch(&repo),
        ],
        &[],
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            Instant::now() < deadline,
            "switch side effect did not enter its gate"
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
    assert!(commit.authorship_log.attestations.iter().any(|file| {
        file.file_path == "tracked.txt"
            && file
                .entries
                .iter()
                .any(|entry| entry.hash.starts_with("h_"))
    }));
}

#[test]
fn switch_discard_without_canonical_object_receipt_keeps_existing_boundary() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    let log = repo.current_working_logs();
    let before = fs::read(log.checkpoints_file()).unwrap();
    repo.git(&["switch", "--discard-changes", &branch(&repo)])
        .unwrap();
    assert!(fs::read(log.checkpoints_file()).unwrap() == before);
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
}

#[test]
fn switch_discard_replacement_tree_preserves_untracked_evidence() {
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
    repo.git(&["switch", "--discard-changes", &branch(&repo)])
        .unwrap();
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
fn switch_discard_missing_reflog_preserves_existing_journal() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    let log = repo.current_working_logs();
    let before = fs::read(log.checkpoints_file()).unwrap();
    repo.git_og(&["config", "core.logAllRefUpdates", "false"])
        .unwrap();
    fs::remove_file(repo.path().join(".git/logs/HEAD")).unwrap();
    switch(&repo, &["--discard-changes", &branch(&repo)]);
    assert!(fs::read(log.checkpoints_file()).unwrap() == before);
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
}

#[test]
fn switch_discard_orphan_preserves_existing_boundary() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    let log = repo.current_working_logs();
    let before = fs::read(log.checkpoints_file()).unwrap();
    switch(&repo, &["--discard-changes", "--orphan", "new-root"]);
    assert_eq!(branch(&repo), "new-root");
    assert!(!repo.path().join("tracked.txt").exists());
    assert!(fs::read(log.checkpoints_file()).unwrap() == before);
}

#[test]
fn switch_discard_v4_skip_worktree_preserves_retained_edit() {
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
    switch(&repo, &["--discard-changes", &branch(&repo)]);
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
fn switch_discard_split_index_skip_worktree_preserves_retained_edit() {
    let repo = TestRepo::new();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.git_og(&["update-index", "--skip-worktree", "tracked.txt"])
        .unwrap();
    repo.git_og(&["update-index", "--split-index"]).unwrap();
    switch(&repo, &["--discard-changes", &branch(&repo)]);
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
fn switch_discard_fsmonitor_preserves_retained_edit() {
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
    switch(&repo, &["--discard-changes", &branch(&repo)]);
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
fn switch_discard_linked_worktree() {
    let repo = TestRepo::new_worktree();
    commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    switch(&repo, &["--discard-changes", &branch(&repo)]);
    assert_untracked_recreation(&repo);
}

#[test]
fn switch_discard_git_process_count_does_not_grow_with_files() {
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
        switch(&repo, &["--discard-changes", &branch(&repo)]);
        repo.sync_daemon_force();
        let commands = fs::read_to_string(&log_path).unwrap();
        assert_eq!(
            commands
                .lines()
                .filter(|command| *command == "cat-file")
                .count(),
            1,
            "switch daemon Git processes: {commands:?}"
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
        "switch Git process count must be independent of file count"
    );
    println!("switch daemon Git process counts for 1 and 8 files: {counts:?}");
}

#[test]
fn switch_discard_preserves_moving_head_behavior() {
    let repo = TestRepo::new();
    let base = commit_base(&repo);
    checkpoint_replacement(&repo, "mock_ai");
    repo.stage_all_and_commit("committed ai").unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["discarded edit".ai()]);
    fs::write(repo.path().join("tracked.txt"), "pending ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "tracked.txt"])
        .unwrap();
    switch(&repo, &["--discard-changes", "--detach", &base]);
    fs::write(repo.path().join("tracked.txt"), "pending ai\n").unwrap();
    repo.stage_all_and_commit("untracked after moving reset")
        .unwrap();
    repo.filename("tracked.txt")
        .assert_committed_lines(lines!["pending ai".unattributed_human()]);
}

#[test]
fn switch_discard_sqlite_and_http_backends() {
    use git_ai::config::{NotesBackendConfig, NotesBackendKind};
    use git_ai::model::authorship_log_serialization::AuthorshipLog;
    use git_ai::model::repository::notes_db::NotesDatabase;
    use git_ai::notes::reference_server::ReferenceServer;

    let server = ReferenceServer::start("127.0.0.1:0").unwrap();
    for kind in [NotesBackendKind::Sqlite, NotesBackendKind::Http] {
        let repo = TestRepo::new_with_daemon_env_and_patch(
            &[("GIT_AI_API_KEY", "switch-backend-test-key")],
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
        switch(&repo, &["--discard-changes", &branch(&repo)]);
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
