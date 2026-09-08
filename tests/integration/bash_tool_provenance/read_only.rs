use super::{
    TestRepo, add_and_commit, assert_no_changes, post_hook, pre_hook, repo_root, run_bash,
};

// ===========================================================================
// Category 7: Read-only commands (should produce NoChanges)
// ===========================================================================

#[test]
fn test_bash_provenance_cat_is_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "readable.txt", "read me", "initial commit");

    pre_hook(&root, "cat-sess", "cat-t1");

    run_bash(&repo, "cat", &["readable.txt"]);

    let post_action = post_hook(&root, "cat-sess", "cat-t1");
    assert_no_changes(&post_action);
}

#[test]
fn test_bash_provenance_ls_is_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "visible.txt", "content", "initial commit");

    pre_hook(&root, "ls-sess", "ls-t1");

    run_bash(&repo, "ls", &["-la"]);

    let post_action = post_hook(&root, "ls-sess", "ls-t1");
    assert_no_changes(&post_action);
}

#[test]
#[cfg(not(target_os = "windows"))] // Windows `find` is not POSIX find
fn test_bash_provenance_find_is_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "src/main.rs", "fn main() {}", "initial commit");

    pre_hook(&root, "find-sess", "find-t1");

    run_bash(&repo, "find", &[".", "-name", "*.rs"]);

    let post_action = post_hook(&root, "find-sess", "find-t1");
    assert_no_changes(&post_action);
}

#[test]
fn test_bash_provenance_grep_is_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(
        &repo,
        "searchable.txt",
        "pattern match here",
        "initial commit",
    );

    pre_hook(&root, "grep-sess", "grep-t1");

    // grep may exit non-zero if no match, so use sh -c with || true
    run_bash(
        &repo,
        "sh",
        &["-c", "grep 'pattern' searchable.txt || true"],
    );

    let post_action = post_hook(&root, "grep-sess", "grep-t1");
    assert_no_changes(&post_action);
}

#[test]
fn test_bash_provenance_wc_is_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "countme.txt", "one\ntwo\nthree\n", "initial commit");

    pre_hook(&root, "wc-sess", "wc-t1");

    run_bash(&repo, "wc", &["-l", "countme.txt"]);

    let post_action = post_hook(&root, "wc-sess", "wc-t1");
    assert_no_changes(&post_action);
}

#[test]
fn test_bash_provenance_head_is_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(
        &repo,
        "longfile.txt",
        "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\n",
        "initial commit",
    );

    pre_hook(&root, "head-sess", "head-t1");

    run_bash(&repo, "head", &["-5", "longfile.txt"]);

    let post_action = post_hook(&root, "head-sess", "head-t1");
    assert_no_changes(&post_action);
}

#[test]
fn test_bash_provenance_diff_is_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "file1.txt", "alpha\nbeta\n", "add file1");
    add_and_commit(&repo, "file2.txt", "alpha\ngamma\n", "add file2");

    pre_hook(&root, "diff-sess", "diff-t1");

    // diff returns non-zero when files differ, so use || true
    run_bash(&repo, "sh", &["-c", "diff file1.txt file2.txt || true"]);

    let post_action = post_hook(&root, "diff-sess", "diff-t1");
    assert_no_changes(&post_action);
}

#[test]
fn test_bash_provenance_git_log_is_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "gitlog-sess", "gitlog-t1");

    repo.git_og(&["log", "--oneline"])
        .expect("git log should succeed");

    let post_action = post_hook(&root, "gitlog-sess", "gitlog-t1");
    assert_no_changes(&post_action);
}

#[test]
fn test_bash_provenance_git_diff_is_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "gitdiff-sess", "gitdiff-t1");

    repo.git_og(&["diff"]).expect("git diff should succeed");

    let post_action = post_hook(&root, "gitdiff-sess", "gitdiff-t1");
    assert_no_changes(&post_action);
}

#[test]
fn test_bash_provenance_git_status_is_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "gitstatus-sess", "gitstatus-t1");

    repo.git_og(&["status"]).expect("git status should succeed");

    let post_action = post_hook(&root, "gitstatus-sess", "gitstatus-t1");
    assert_no_changes(&post_action);
}

#[test]
fn test_bash_provenance_compound_readonly() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "compound-sess", "compound-t1");

    run_bash(&repo, "sh", &["-c", "pwd && ls"]);

    let post_action = post_hook(&root, "compound-sess", "compound-t1");
    assert_no_changes(&post_action);
}
