use super::{
    ExpectedLineExt, TestRepo, assert_human_records_have_email, assert_session_authors_have_email,
    fs,
};

/// Verify that SessionRecord.human_author includes email after checkout carryover.
/// Exercises daemon.rs working log carryover path (checkout_hooks → restore_working_log_carryover).
#[test]
fn test_checkout_carryover_preserves_author_email_in_session() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("work.txt");

    fs::write(repo.path().join("README.md"), "init\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    repo.git(&["branch", "feature"]).unwrap();

    // Create AI checkpoint on main (uncommitted)
    repo.git_ai(&["checkpoint", "human", "work.txt"]).unwrap();
    fs::write(&file_path, "AI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "work.txt"]).unwrap();

    // Checkout feature — working log carries over
    repo.git(&["checkout", "feature"]).unwrap();

    // Commit on feature branch
    repo.stage_all_and_commit("commit on feature").unwrap();

    let mut file = repo.filename("work.txt");
    file.assert_committed_lines(crate::lines!["AI line".ai()]);

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_session_authors_have_email(&repo, &sha);
}

/// Verify that SessionRecord.human_author includes email after `git switch` carryover.
/// Exercises daemon.rs switch_hooks → restore_working_log_carryover path.
#[test]
fn test_switch_carryover_preserves_author_email_in_session() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("work.txt");

    fs::write(repo.path().join("README.md"), "init\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    repo.git(&["branch", "feature"]).unwrap();

    // Create AI checkpoint on main (uncommitted)
    repo.git_ai(&["checkpoint", "human", "work.txt"]).unwrap();
    fs::write(&file_path, "AI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "work.txt"]).unwrap();

    // Switch to feature — working log carries over
    repo.git(&["switch", "feature"]).unwrap();

    // Commit on feature branch
    repo.stage_all_and_commit("commit on feature").unwrap();

    let mut file = repo.filename("work.txt");
    file.assert_committed_lines(crate::lines!["AI line".ai()]);

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_session_authors_have_email(&repo, &sha);
}

/// Verify that SessionRecord.human_author includes email after rebase rewrites the note.
/// Exercises daemon.rs apply_rewrite_prerequisites → post_commit path.
#[test]
fn test_rebase_rewrite_preserves_author_email_in_session() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("code.rs");

    // Base commit
    fs::write(&file_path, "fn base() {}\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();

    // AI commit on top
    repo.git_ai(&["checkpoint", "human", "code.rs"]).unwrap();
    fs::write(&file_path, "fn base() {}\nfn ai() {}\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "code.rs"]).unwrap();
    repo.stage_all_and_commit("ai commit").unwrap();

    let mut file = repo.filename("code.rs");
    file.assert_committed_lines(crate::lines![
        "fn base() {}".unattributed_human(),
        "fn ai() {}".ai(),
    ]);

    // Create a new base commit on a side branch to rebase onto
    repo.git(&["checkout", "-b", "new-base", "HEAD~1"]).unwrap();
    fs::write(repo.path().join("other.txt"), "other\n").unwrap();
    repo.stage_all_and_commit("new base commit").unwrap();
    let new_base = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Go back to the AI commit's branch and rebase
    repo.git(&["checkout", "-"]).unwrap();
    repo.git(&["rebase", &new_base]).unwrap();

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_session_authors_have_email(&repo, &sha);
}

/// Verify that HumanRecord.author includes email after rebase rewrites the note.
#[test]
fn test_rebase_rewrite_preserves_author_email_in_human_record() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("code.rs");

    // Base commit
    fs::write(&file_path, "fn base() {}\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();

    // Known-human commit on top
    fs::write(&file_path, "fn base() {}\nfn human() {}\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "code.rs"])
        .unwrap();
    repo.stage_all_and_commit("human commit").unwrap();

    let mut file = repo.filename("code.rs");
    file.assert_committed_lines(crate::lines![
        "fn base() {}".unattributed_human(),
        "fn human() {}".human(),
    ]);

    // Create a new base commit on a side branch
    repo.git(&["checkout", "-b", "new-base", "HEAD~1"]).unwrap();
    fs::write(repo.path().join("other.txt"), "other\n").unwrap();
    repo.stage_all_and_commit("new base commit").unwrap();
    let new_base = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Go back and rebase
    repo.git(&["checkout", "-"]).unwrap();
    repo.git(&["rebase", &new_base]).unwrap();

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_human_records_have_email(&repo, &sha);
}

/// Verify that `git-ai status` implicit checkpoint flows through to email in SessionRecord.
/// Exercises status.rs → checkpoint::run → post_commit path.
#[test]
fn test_status_checkpoint_preserves_author_email_in_session() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("app.py");

    // Base commit
    fs::write(&file_path, "print('hello')\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();

    // AI edits
    repo.git_ai(&["checkpoint", "human", "app.py"]).unwrap();
    fs::write(&file_path, "print('hello')\nprint('ai')\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "app.py"]).unwrap();

    // Run git-ai status (triggers implicit human checkpoint internally)
    let _ = repo.git_ai(&["status", "--json"]);

    // Commit after status
    repo.stage_all_and_commit("post-status commit").unwrap();

    let mut file = repo.filename("app.py");
    file.assert_committed_lines(crate::lines![
        "print('hello')".unattributed_human(),
        "print('ai')".ai(),
    ]);

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_session_authors_have_email(&repo, &sha);
}

crate::reuse_tests_in_worktree!(
    test_checkout_carryover_preserves_author_email_in_session,
    test_switch_carryover_preserves_author_email_in_session,
    test_rebase_rewrite_preserves_author_email_in_session,
    test_rebase_rewrite_preserves_author_email_in_human_record,
    test_status_checkpoint_preserves_author_email_in_session,
);
