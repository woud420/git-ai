use super::{add_and_commit, assert_read_only_command, run_bash};

// ===========================================================================
// Category 7: Read-only commands (should produce NoChanges)
// ===========================================================================

#[test]
fn test_bash_provenance_cat_is_readonly() {
    assert_read_only_command(
        ("cat-sess", "cat-t1"),
        |repo| add_and_commit(repo, "readable.txt", "read me", "initial commit"),
        |repo| run_bash(repo, "cat", &["readable.txt"]),
    );
}

#[test]
fn test_bash_provenance_ls_is_readonly() {
    assert_read_only_command(
        ("ls-sess", "ls-t1"),
        |repo| add_and_commit(repo, "visible.txt", "content", "initial commit"),
        |repo| run_bash(repo, "ls", &["-la"]),
    );
}

#[test]
#[cfg(not(target_os = "windows"))] // Windows `find` is not POSIX find
fn test_bash_provenance_find_is_readonly() {
    assert_read_only_command(
        ("find-sess", "find-t1"),
        |repo| add_and_commit(repo, "src/main.rs", "fn main() {}", "initial commit"),
        |repo| run_bash(repo, "find", &[".", "-name", "*.rs"]),
    );
}

#[test]
fn test_bash_provenance_grep_is_readonly() {
    assert_read_only_command(
        ("grep-sess", "grep-t1"),
        |repo| {
            add_and_commit(
                repo,
                "searchable.txt",
                "pattern match here",
                "initial commit",
            )
        },
        |repo| {
            // grep may exit non-zero if no match, so use sh -c with || true
            run_bash(repo, "sh", &["-c", "grep 'pattern' searchable.txt || true"])
        },
    );
}

#[test]
fn test_bash_provenance_wc_is_readonly() {
    assert_read_only_command(
        ("wc-sess", "wc-t1"),
        |repo| add_and_commit(repo, "countme.txt", "one\ntwo\nthree\n", "initial commit"),
        |repo| run_bash(repo, "wc", &["-l", "countme.txt"]),
    );
}

#[test]
fn test_bash_provenance_head_is_readonly() {
    assert_read_only_command(
        ("head-sess", "head-t1"),
        |repo| {
            add_and_commit(
                repo,
                "longfile.txt",
                "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\n",
                "initial commit",
            )
        },
        |repo| run_bash(repo, "head", &["-5", "longfile.txt"]),
    );
}

#[test]
fn test_bash_provenance_diff_is_readonly() {
    assert_read_only_command(
        ("diff-sess", "diff-t1"),
        |repo| {
            add_and_commit(repo, "file1.txt", "alpha\nbeta\n", "add file1");
            add_and_commit(repo, "file2.txt", "alpha\ngamma\n", "add file2")
        },
        |repo| {
            // diff returns non-zero when files differ, so use || true
            run_bash(repo, "sh", &["-c", "diff file1.txt file2.txt || true"])
        },
    );
}

#[test]
fn test_bash_provenance_git_log_is_readonly() {
    assert_read_only_command(
        ("gitlog-sess", "gitlog-t1"),
        |repo| add_and_commit(repo, "init.txt", "seed", "initial commit"),
        |repo| {
            repo.git_og(&["log", "--oneline"])
                .expect("git log should succeed")
        },
    );
}

#[test]
fn test_bash_provenance_git_diff_is_readonly() {
    assert_read_only_command(
        ("gitdiff-sess", "gitdiff-t1"),
        |repo| add_and_commit(repo, "init.txt", "seed", "initial commit"),
        |repo| repo.git_og(&["diff"]).expect("git diff should succeed"),
    );
}

#[test]
fn test_bash_provenance_git_status_is_readonly() {
    assert_read_only_command(
        ("gitstatus-sess", "gitstatus-t1"),
        |repo| add_and_commit(repo, "init.txt", "seed", "initial commit"),
        |repo| repo.git_og(&["status"]).expect("git status should succeed"),
    );
}

#[test]
fn test_bash_provenance_compound_readonly() {
    assert_read_only_command(
        ("compound-sess", "compound-t1"),
        |repo| add_and_commit(repo, "init.txt", "seed", "initial commit"),
        |repo| run_bash(repo, "sh", &["-c", "pwd && ls"]),
    );
}
