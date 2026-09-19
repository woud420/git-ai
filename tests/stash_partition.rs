#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;

fn checkpoint_write(repo: &TestRepo, path: &str, text: &str) {
    repo.git_ai(&["checkpoint", "human", path]).unwrap();
    fs::write(repo.path().join(path), text).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", path]).unwrap();
}

fn seeded_repo() -> TestRepo {
    seed(TestRepo::new())
}

fn seed(repo: TestRepo) -> TestRepo {
    for path in ["first.txt", "second.txt"] {
        fs::write(repo.path().join(path), "base\n").unwrap();
    }
    repo.git(&["add", "."]).unwrap();
    repo.git(&["commit", "-m", "base"]).unwrap();
    assert_tracked(&repo, false, false);
    repo
}

fn assert_tracked(repo: &TestRepo, first_ai: bool, second_ai: bool) {
    for (path, ai) in [("first.txt", first_ai), ("second.txt", second_ai)] {
        let mut expected = lines!["base".unattributed_human()];
        if ai {
            expected.push("AI addition".ai());
        }
        repo.filename(path).assert_committed_lines(expected);
    }
}

#[test]
fn keep_index_stash_preserves_staged_ai_and_restores_unstaged_ai() {
    let repo = seeded_repo();
    checkpoint_write(&repo, "first.txt", "base\nAI addition\n");
    repo.git(&["add", "first.txt"]).unwrap();
    checkpoint_write(&repo, "second.txt", "base\nAI addition\n");
    repo.git(&["stash", "push", "--keep-index"]).unwrap();
    assert_eq!(
        fs::read_to_string(repo.path().join("first.txt")).unwrap(),
        "base\nAI addition\n"
    );
    repo.commit("commit staged AI left by stash").unwrap();
    assert_tracked(&repo, true, false);
    repo.git(&["stash", "pop"]).unwrap();
    repo.stage_all_and_commit("commit restored unstaged AI")
        .unwrap();
    assert_tracked(&repo, true, true);
}

#[test]
fn path_limited_keep_index_preserves_both_the_staged_and_unselected_ai() {
    let repo = seeded_repo();
    checkpoint_write(&repo, "first.txt", "base\nAI addition\n");
    repo.git(&["add", "first.txt"]).unwrap();
    checkpoint_write(&repo, "second.txt", "base\nAI addition\n");
    repo.git(&["stash", "push", "--keep-index", "--", "first.txt"])
        .unwrap();
    repo.stage_all_and_commit("commit evidence left live by path-limited stash")
        .unwrap();
    assert_tracked(&repo, true, true);
}

#[test]
fn ordinary_stash_keeps_untracked_ai_evidence_that_git_leaves_on_disk() {
    let repo = seeded_repo();
    checkpoint_write(&repo, "first.txt", "base\nAI addition\n");
    checkpoint_write(&repo, "new.txt", "untracked AI\n");
    repo.git(&["stash", "push"]).unwrap();
    assert!(repo.path().join("new.txt").exists());
    repo.stage_all_and_commit("commit unstashed new file")
        .unwrap();
    assert_tracked(&repo, false, false);
    repo.filename("new.txt")
        .assert_committed_lines(lines!["untracked AI".ai()]);
    repo.git(&["stash", "pop"]).unwrap();
    repo.stage_all_and_commit("commit restored tracked AI")
        .unwrap();
    assert_tracked(&repo, true, false);
    repo.filename("new.txt")
        .assert_committed_lines(lines!["untracked AI".ai()]);
}

#[test]
fn staged_only_stash_preserves_unstaged_ai_for_its_own_commit() {
    let repo = seeded_repo();
    checkpoint_write(&repo, "first.txt", "base\nAI addition\n");
    repo.git(&["add", "first.txt"]).unwrap();
    checkpoint_write(&repo, "second.txt", "base\nAI addition\n");
    repo.git(&["stash", "push", "--staged"]).unwrap();
    assert_eq!(
        fs::read_to_string(repo.path().join("second.txt")).unwrap(),
        "base\nAI addition\n"
    );
    repo.stage_all_and_commit("commit unstaged AI left by staged-only stash")
        .unwrap();
    assert_tracked(&repo, false, true);
    repo.git(&["stash", "pop"]).unwrap();
    repo.stage_all_and_commit("commit stashed staged AI")
        .unwrap();
    assert_tracked(&repo, true, true);
}

#[test]
fn keep_index_stash_retains_an_earlier_checkpoint_replaced_in_the_same_file() {
    let repo = seeded_repo();
    checkpoint_write(&repo, "first.txt", "base\nearlier AI\n");
    repo.git(&["add", "first.txt"]).unwrap();
    checkpoint_write(&repo, "first.txt", "base\nlater AI\n");
    repo.git(&["stash", "push", "--keep-index"]).unwrap();
    repo.commit("commit earlier staged checkpoint").unwrap();
    repo.filename("first.txt")
        .assert_committed_lines(lines!["base".unattributed_human(), "earlier AI".ai()]);
    repo.filename("second.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
}

#[test]
fn keep_index_stash_splits_staged_and_unstaged_ai_lines_in_one_file() {
    let repo = seeded_repo();
    checkpoint_write(&repo, "first.txt", "base\nAI addition\n");
    repo.git(&["add", "first.txt"]).unwrap();
    checkpoint_write(&repo, "first.txt", "base\nAI addition\nunstaged AI\n");
    repo.git(&["stash", "push", "--keep-index"]).unwrap();
    repo.commit("commit staged AI lines").unwrap();
    assert_tracked(&repo, true, false);
    repo.git(&["checkout", "-b", "restore-saved-stash", "HEAD^"])
        .unwrap();
    repo.git(&["stash", "pop"]).unwrap();
    repo.stage_all_and_commit("commit restored AI lines")
        .unwrap();
    repo.filename("first.txt").assert_committed_lines(lines![
        "base".unattributed_human(),
        "AI addition".ai(),
        "unstaged AI".ai()
    ]);
    repo.filename("second.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
}

#[test]
fn include_untracked_stash_restores_new_file_ai_from_the_third_parent() {
    let repo = seeded_repo();
    checkpoint_write(&repo, "first.txt", "base\nAI addition\n");
    checkpoint_write(&repo, "new.txt", "new AI\n");
    repo.git(&["stash", "push", "--include-untracked"]).unwrap();
    assert!(!repo.path().join("new.txt").exists());
    repo.git(&["stash", "pop"]).unwrap();
    repo.stage_all_and_commit("commit restored tracked and new AI")
        .unwrap();
    assert_tracked(&repo, true, false);
    repo.filename("new.txt")
        .assert_committed_lines(lines!["new AI".ai()]);
}

#[test]
fn keep_index_stash_restores_known_human_checkpoint_identity() {
    let repo = seeded_repo();
    fs::write(repo.path().join("first.txt"), "base\nknown human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "first.txt"])
        .unwrap();
    repo.git(&["add", "first.txt"]).unwrap();
    checkpoint_write(&repo, "first.txt", "base\nlater AI\n");
    repo.git(&["stash", "push", "-k"]).unwrap();
    repo.commit("commit retained known human").unwrap();
    repo.filename("first.txt")
        .assert_committed_lines(lines!["base".unattributed_human(), "known human".human()]);
    repo.filename("second.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    let head = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    let log = repo.require_authorship_log(head.trim());
    assert!(
        log.attestations
            .iter()
            .filter(|f| f.file_path == "first.txt")
            .flat_map(|f| &f.entries)
            .any(|entry| log.metadata.humans.contains_key(&entry.hash)
                && entry.line_ranges.iter().any(|range| range.contains(2)))
    );
}

#[test]
fn keep_index_stash_does_not_label_uncheckpointed_index_text_as_ai() {
    let repo = seeded_repo();
    fs::write(
        repo.path().join("first.txt"),
        "base\nuntracked staged text\n",
    )
    .unwrap();
    repo.git(&["add", "first.txt"]).unwrap();
    checkpoint_write(&repo, "first.txt", "base\nlater AI\n");
    repo.git(&["stash", "push", "--keep-index"]).unwrap();
    repo.commit("commit untracked index text").unwrap();
    repo.filename("first.txt").assert_committed_lines(lines![
        "base".unattributed_human(),
        "untracked staged text".unattributed_human()
    ]);
    repo.filename("second.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
}

#[test]
fn keep_index_stash_recovers_staged_ai_after_a_later_untracked_checkpoint() {
    let repo = seeded_repo();
    checkpoint_write(&repo, "first.txt", "base\nearlier AI\n");
    repo.git(&["add", "first.txt"]).unwrap();
    fs::write(repo.path().join("first.txt"), "base\nlater untracked\n").unwrap();
    repo.git_ai(&["checkpoint", "human", "first.txt"]).unwrap();
    repo.git(&["stash", "push", "--keep-index"]).unwrap();
    repo.commit("commit retained AI before untracked checkpoint")
        .unwrap();
    repo.filename("first.txt")
        .assert_committed_lines(lines!["base".unattributed_human(), "earlier AI".ai()]);
    repo.filename("second.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
}

#[test]
fn sqlite_keep_index_stash_preserves_staged_and_restored_ai() {
    let repo = seed(TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.notes_backend = Some(git_ai::config::NotesBackendConfig {
            kind: git_ai::config::NotesBackendKind::Sqlite,
            backend_url: None,
        });
    }));
    checkpoint_write(&repo, "first.txt", "base\nAI addition\n");
    repo.git(&["add", "first.txt"]).unwrap();
    checkpoint_write(&repo, "second.txt", "base\nAI addition\n");
    repo.git(&["stash", "push", "-km", "keep index"]).unwrap();
    repo.git(&["commit", "-m", "commit SQLite staged evidence"])
        .unwrap();
    assert_tracked(&repo, true, false);
    repo.git(&["stash", "pop"]).unwrap();
    repo.git(&["add", "."]).unwrap();
    repo.git(&["commit", "-m", "commit SQLite restored evidence"])
        .unwrap();
    assert_tracked(&repo, true, true);
}

#[test]
fn delayed_keep_index_partition_uses_stash_trees_after_head_and_worktree_advance() {
    let dir = tempfile::tempdir().unwrap();
    let gate = dir.path().join("stash-gate");
    fs::write(&gate, "").unwrap();
    let spec = format!("stash={}", gate.display());
    let repo = seed(TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND",
        &spec,
    )]));
    checkpoint_write(&repo, "first.txt", "base\nAI addition\n");
    repo.git(&["add", "first.txt"]).unwrap();
    checkpoint_write(&repo, "second.txt", "base\nAI addition\n");
    repo.git(&["stash", "push", "--keep-index"]).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "stash effect never entered gate"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    repo.git_without_test_sync_for_test(
        &["commit", "-m", "commit while stash processing is gated"],
        &[],
    )
    .unwrap();
    fs::write(
        repo.path().join("first.txt"),
        "later unrelated worktree contents\n",
    )
    .unwrap();
    fs::remove_file(&gate).unwrap();
    repo.sync_daemon_force();
    fs::write(repo.path().join("first.txt"), "base\nAI addition\n").unwrap();
    assert_tracked(&repo, true, false);
    repo.git(&["stash", "pop"]).unwrap();
    repo.stage_all_and_commit("commit restored evidence after delayed partition")
        .unwrap();
    assert_tracked(&repo, true, true);
}

#[test]
fn stash_partition_git_spawns_do_not_scale_with_file_count() {
    fn run(count: usize) -> usize {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("spawns.jsonl");
        let repo = TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())]);
        for index in 0..count {
            fs::write(repo.path().join(format!("file-{index}.txt")), "base\n").unwrap();
        }
        repo.stage_all_and_commit("base").unwrap();
        for index in 0..count {
            let path = format!("file-{index}.txt");
            repo.filename(&path)
                .assert_committed_lines(lines!["base".unattributed_human()]);
            checkpoint_write(&repo, &path, "base\nAI addition\n");
        }
        repo.git(&["add", "."]).unwrap();
        repo.sync_daemon();
        let before = fs::read_to_string(&log).unwrap().lines().count();
        repo.git(&["stash", "push", "--keep-index"]).unwrap();
        repo.sync_daemon();
        let spawns = fs::read_to_string(&log).unwrap().lines().count() - before;
        repo.commit("commit retained AI in all files").unwrap();
        for index in 0..count {
            repo.filename(&format!("file-{index}.txt"))
                .assert_committed_lines(lines!["base".unattributed_human(), "AI addition".ai()]);
        }
        spawns
    }
    let small = run(2);
    let large = run(8);
    eprintln!("stash partition spawns: 2 files={small}, 8 files={large}");
    assert!(
        large <= small + 2,
        "stash git work scales with file count: {small} -> {large}"
    );
}

reuse_tests_in_worktree!(
    keep_index_stash_preserves_staged_ai_and_restores_unstaged_ai,
    path_limited_keep_index_preserves_both_the_staged_and_unselected_ai,
    ordinary_stash_keeps_untracked_ai_evidence_that_git_leaves_on_disk,
    staged_only_stash_preserves_unstaged_ai_for_its_own_commit,
    keep_index_stash_retains_an_earlier_checkpoint_replaced_in_the_same_file,
    keep_index_stash_splits_staged_and_unstaged_ai_lines_in_one_file,
    include_untracked_stash_restores_new_file_ai_from_the_third_parent,
    keep_index_stash_restores_known_human_checkpoint_identity,
    keep_index_stash_does_not_label_uncheckpointed_index_text_as_ai,
    keep_index_stash_recovers_staged_ai_after_a_later_untracked_checkpoint,
);

#[test]
fn stash_partition_rejects_a_tampered_checkpoint_journal_without_consuming_evidence() {
    let repo = seeded_repo();
    checkpoint_write(&repo, "first.txt", "base\nAI addition\n");
    let working_log = repo.current_working_logs();
    let journal = working_log.checkpoints_file();
    let original = fs::read_to_string(&journal).unwrap();
    let tampered = original.replace("mock_ai", "tampered-agent");
    assert_ne!(original, tampered);
    fs::write(&journal, &tampered).unwrap();
    let repository =
        git_ai::operations::git::repository::find_repository_in_path(repo.path().to_str().unwrap())
            .unwrap();
    let head = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    let error = git_ai::operations::authorship::rewrite_stash::handle_stash_create(
        &repository,
        &"1".repeat(40),
        head.trim(),
        vec!["first.txt".to_owned()],
        true,
    )
    .expect_err("stash partition must not launder a corrupted checkpoint");
    assert!(error.to_string().contains("checksum"), "{error}");
    assert_eq!(fs::read_to_string(&journal).unwrap(), tampered);
}

#[test]
fn stash_does_not_resurrect_ai_for_uncheckpointed_recreation_of_old_text() {
    let repo = seeded_repo();
    checkpoint_write(&repo, "first.txt", "base\nreused text\n");
    fs::write(repo.path().join("first.txt"), "base\n").unwrap();
    repo.git_ai(&["checkpoint", "human", "first.txt"]).unwrap();
    fs::write(repo.path().join("first.txt"), "base\nreused text\n").unwrap();
    repo.git(&["stash", "push"]).unwrap();
    repo.git(&["stash", "pop"]).unwrap();
    repo.stage_all_and_commit("commit untracked recreated text")
        .unwrap();
    repo.filename("first.txt").assert_committed_lines(lines![
        "base".unattributed_human(),
        "reused text".unattributed_human()
    ]);
    repo.filename("second.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
}
