use super::{
    TestRepo, assert_ai_authorship_note, assert_no_ai_authorship_for_commit,
    assert_no_authorship_note, assert_traced_commit_has_no_ai_authorship, cold_repo,
    raw_commit_file, raw_git, raw_head, read_file, run_traced_git, start_cold_daemon,
};

#[test]
fn test_cold_repo_first_traced_commit_is_processed() {
    let mut repo = cold_repo();
    let raw_first = raw_commit_file(&repo, "history.txt", "base\n", "raw base");
    let raw_second = raw_commit_file(&repo, "history.txt", "base\nraw\n", "raw second");
    repo.write_file("traced.txt", "first traced commit\n");
    raw_git(&repo, &["add", "traced.txt"]);

    start_cold_daemon(&mut repo);
    run_traced_git(&repo, &["commit", "-m", "first traced commit"]);

    let head = raw_head(&repo);
    assert_ne!(head, raw_second);
    assert_eq!(read_file(&repo, "traced.txt"), "first traced commit\n");
    assert_no_ai_authorship_for_commit(&repo, &raw_first);
    assert_no_ai_authorship_for_commit(&repo, &raw_second);
    assert_no_ai_authorship_for_commit(&repo, &head);
}

#[test]
fn test_cold_repo_commit_message_trailing_whitespace_preserves_ai_authorship() {
    let mut repo = cold_repo();
    raw_commit_file(&repo, "tracked.txt", "base\n", "raw base");

    start_cold_daemon(&mut repo);
    repo.write_file("tracked.txt", "base\nAI line\n");
    repo.git_ai(&["checkpoint", "mock_ai", "tracked.txt"])
        .expect("mock_ai checkpoint should succeed");
    repo.git(&["add", "tracked.txt"])
        .expect("staging AI change should succeed");
    run_traced_git(&repo, &["commit", "-m", "AI change "]);

    assert_ai_authorship_note(&repo, &raw_head(&repo));
    let stats = repo.stats().expect("commit stats should be available");
    assert_eq!(stats.ai_additions, 1);
    assert_eq!(stats.unknown_additions, 0);
}

#[test]
fn test_traced_commit_after_untraced_head_move_creates_authorship_note() {
    let repo = TestRepo::new_dedicated_daemon();

    repo.write_file("base.txt", "base\n");
    repo.git(&["add", "base.txt"]).unwrap();
    run_traced_git(&repo, &["commit", "-m", "traced base"]);
    let traced_base = raw_head(&repo);
    assert_traced_commit_has_no_ai_authorship(&repo, &traced_base);

    let raw_unseen = raw_commit_file(&repo, "raw.txt", "raw unseen\n", "raw unseen");
    assert_no_ai_authorship_for_commit(&repo, &raw_unseen);

    repo.write_file("next.txt", "next traced\n");
    repo.git(&["add", "next.txt"]).unwrap();
    run_traced_git(&repo, &["commit", "-m", "traced after raw"]);
    let traced_after_raw = raw_head(&repo);

    assert_traced_commit_has_no_ai_authorship(&repo, &traced_after_raw);
}

#[test]
fn test_traced_commit_after_untraced_duplicate_message_head_move_notes_traced_commit() {
    let repo = TestRepo::new_dedicated_daemon();

    repo.write_file("base.txt", "base\n");
    repo.git(&["add", "base.txt"]).unwrap();
    run_traced_git(&repo, &["commit", "-m", "traced base"]);
    let traced_base = raw_head(&repo);
    assert_traced_commit_has_no_ai_authorship(&repo, &traced_base);

    let raw_unseen = raw_commit_file(&repo, "raw.txt", "raw unseen\n", "same message");
    assert_no_authorship_note(&repo, &raw_unseen);

    repo.write_file("next.txt", "next traced\n");
    repo.git(&["add", "next.txt"]).unwrap();
    run_traced_git(&repo, &["commit", "-m", "same message"]);
    let traced_after_raw = raw_head(&repo);

    assert_no_authorship_note(&repo, &raw_unseen);
    assert_traced_commit_has_no_ai_authorship(&repo, &traced_after_raw);
}

#[test]
fn test_cold_repo_first_traced_amend_is_processed() {
    let mut repo = cold_repo();
    let original = raw_commit_file(&repo, "amend.txt", "before\n", "raw before amend");
    repo.write_file("amend.txt", "before\namended\n");
    raw_git(&repo, &["add", "amend.txt"]);

    start_cold_daemon(&mut repo);
    run_traced_git(&repo, &["commit", "--amend", "--no-edit"]);

    let amended = raw_head(&repo);
    assert_ne!(amended, original);
    assert_eq!(read_file(&repo, "amend.txt"), "before\namended\n");
    assert_no_ai_authorship_for_commit(&repo, &amended);
}

#[test]
fn test_cold_repo_first_traced_soft_reset_is_processed() {
    let mut repo = cold_repo();
    let first = raw_commit_file(&repo, "reset.txt", "one\n", "raw reset base");
    let second = raw_commit_file(&repo, "reset.txt", "one\ntwo\n", "raw reset advance");

    start_cold_daemon(&mut repo);
    run_traced_git(&repo, &["reset", "--soft", &first]);

    assert_eq!(raw_head(&repo), first);
    assert_eq!(read_file(&repo, "reset.txt"), "one\ntwo\n");
    let staged = raw_git(&repo, &["diff", "--cached", "--name-only"]);
    assert!(
        staged.lines().any(|line| line == "reset.txt"),
        "soft reset should leave reset.txt staged, got: {}",
        staged
    );
    assert_no_ai_authorship_for_commit(&repo, &second);
}

#[test]
fn test_cold_repo_first_traced_cherry_pick_is_processed() {
    let mut repo = cold_repo();
    raw_commit_file(&repo, "base.txt", "base\n", "raw base");
    raw_git(&repo, &["branch", "-M", "main"]);
    raw_git(&repo, &["checkout", "-b", "feature"]);
    let source = raw_commit_file(&repo, "picked.txt", "picked\n", "raw picked source");
    raw_git(&repo, &["checkout", "main"]);
    raw_commit_file(&repo, "main.txt", "main\n", "raw main advance");

    start_cold_daemon(&mut repo);
    run_traced_git(&repo, &["cherry-pick", &source]);

    let picked = raw_head(&repo);
    assert_ne!(picked, source);
    assert_eq!(read_file(&repo, "picked.txt"), "picked\n");
    assert_no_ai_authorship_for_commit(&repo, &picked);
}

#[test]
fn test_cold_repo_first_traced_stash_pop_is_processed() {
    let mut repo = cold_repo();
    raw_commit_file(&repo, "stash.txt", "base\n", "raw base");
    repo.write_file("stash.txt", "base\nstashed\n");
    raw_git(&repo, &["stash", "push", "-m", "raw stash"]);
    assert_eq!(read_file(&repo, "stash.txt"), "base\n");

    start_cold_daemon(&mut repo);
    run_traced_git(&repo, &["stash", "pop"]);

    assert_eq!(read_file(&repo, "stash.txt"), "base\nstashed\n");
    let stash_list = raw_git(&repo, &["stash", "list"]);
    assert!(
        stash_list.trim().is_empty(),
        "stash pop should drop the raw stash, got: {}",
        stash_list
    );
}

crate::reuse_tests_in_worktree!(
    test_cold_repo_first_traced_commit_is_processed,
    test_cold_repo_commit_message_trailing_whitespace_preserves_ai_authorship,
    test_traced_commit_after_untraced_head_move_creates_authorship_note,
    test_traced_commit_after_untraced_duplicate_message_head_move_notes_traced_commit,
    test_cold_repo_first_traced_amend_is_processed,
    test_cold_repo_first_traced_soft_reset_is_processed,
    test_cold_repo_first_traced_cherry_pick_is_processed,
    test_cold_repo_first_traced_stash_pop_is_processed,
);
