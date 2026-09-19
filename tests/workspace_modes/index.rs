use super::*;

#[test]
fn add_update_preserves_untracked_ai_for_a_later_commit() {
    let repo = repo_with_pending_ai();
    write_ai(&repo, "seed.txt", "updated AI\n");
    repo.git(&["add", "-u"]).unwrap();
    repo.commit("tracked update").unwrap();
    repo.filename("seed.txt")
        .assert_committed_lines(lines!["updated AI".ai()]);
    assert!(repo.git(&["show", "HEAD:pending.txt"]).is_err());
    repo.stage_all_and_commit("untracked remainder").unwrap();
    repo.filename("seed.txt")
        .assert_committed_lines(lines!["updated AI".ai()]);
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["pending AI".ai()]);
}

#[test]
fn add_nul_pathspec_preserves_unstaged_ai_for_a_later_commit() {
    let repo = repo_with_pending_ai();
    write_ai(&repo, "selected name.txt", "selected AI\n");
    let temp = tempfile::tempdir().unwrap();
    let paths = temp.path().join("paths.nul");
    fs::write(&paths, b"selected name.txt\0").unwrap();
    repo.git(&[
        "add",
        &format!("--pathspec-from-file={}", paths.display()),
        "--pathspec-file-nul",
    ])
    .unwrap();
    repo.commit("selected file").unwrap();
    repo.filename("seed.txt")
        .assert_committed_lines(lines!["seed".unattributed_human()]);
    repo.filename("selected name.txt")
        .assert_committed_lines(lines!["selected AI".ai()]);
    assert!(repo.git(&["show", "HEAD:pending.txt"]).is_err());
    commit_and_assert_pending(&repo, "remaining file");
    repo.filename("selected name.txt")
        .assert_committed_lines(lines!["selected AI".ai()]);
}

#[test]
fn add_dry_run_preserves_index_and_pending_ai() {
    let repo = repo_with_pending_ai();
    repo.git(&["add", "--dry-run", "--", "pending.txt"])
        .unwrap();
    assert!(repo.git_og(&["diff", "--cached", "--quiet"]).is_ok());
    commit_and_assert_pending(&repo, "after dry-run staging");
}
