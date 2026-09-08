use super::{
    Duration, TestRepo, add_and_commit, assert_checkpoint_contains, assert_checkpoint_excludes,
    post_hook, pre_hook, repo_root, run_bash, snapshot, thread,
};

// ===========================================================================
// Category 1: File creation commands
// ===========================================================================

#[test]
fn test_bash_provenance_echo_redirect_creates_file() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "echo-sess", "echo-t1");

    run_bash(&repo, "sh", &["-c", "echo 'hello world' > created.txt"]);

    let post_action = post_hook(&root, "echo-sess", "echo-t1");
    assert_checkpoint_contains(&post_action, "created.txt");
}

#[test]
fn test_bash_provenance_printf_redirect_creates_file() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "printf-sess", "printf-t1");

    run_bash(
        &repo,
        "sh",
        &["-c", "printf 'formatted content' > printf_out.txt"],
    );

    let post_action = post_hook(&root, "printf-sess", "printf-t1");
    assert_checkpoint_contains(&post_action, "printf_out.txt");
}

#[test]
fn test_bash_provenance_heredoc_creates_file() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "heredoc-sess", "heredoc-t1");

    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            "cat > heredoc.txt <<'EOF'\nheredoc content\nline two\nEOF",
        ],
    );

    let post_action = post_hook(&root, "heredoc-sess", "heredoc-t1");
    assert_checkpoint_contains(&post_action, "heredoc.txt");
}

#[test]
fn test_bash_provenance_touch_creates_empty_file() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "touch-sess", "touch-t1");

    run_bash(&repo, "touch", &["newfile.txt"]);

    let post_action = post_hook(&root, "touch-sess", "touch-t1");
    assert_checkpoint_contains(&post_action, "newfile.txt");
}

#[test]
fn test_bash_provenance_cp_creates_copy() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "existing.txt", "original content", "initial commit");

    pre_hook(&root, "cp-sess", "cp-t1");

    run_bash(&repo, "cp", &["existing.txt", "copy.txt"]);

    let post_action = post_hook(&root, "cp-sess", "cp-t1");
    assert_checkpoint_contains(&post_action, "copy.txt");
    assert_checkpoint_excludes(&post_action, "existing.txt");
}

#[test]
fn test_bash_provenance_tee_creates_file() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "tee-sess", "tee-t1");

    run_bash(
        &repo,
        "sh",
        &["-c", "echo content | tee output.txt > /dev/null"],
    );

    let post_action = post_hook(&root, "tee-sess", "tee-t1");
    assert_checkpoint_contains(&post_action, "output.txt");
}

#[test]
fn test_bash_provenance_nested_directory_creation() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "nested-sess", "nested-t1");

    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            "mkdir -p src/deep/nested && touch src/deep/nested/mod.rs",
        ],
    );

    let post_action = post_hook(&root, "nested-sess", "nested-t1");
    assert_checkpoint_contains(&post_action, "mod.rs");
}

// ===========================================================================
// Category 2: File modification commands
// ===========================================================================

#[test]
fn test_bash_provenance_sed_in_place_edit() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "target.txt", "old value here", "initial commit");

    pre_hook(&root, "sed-sess", "sed-t1");

    thread::sleep(Duration::from_millis(50));
    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            "sed -i.bak 's/old/new/g' target.txt && rm -f target.txt.bak",
        ],
    );

    let post_action = post_hook(&root, "sed-sess", "sed-t1");
    assert_checkpoint_contains(&post_action, "target.txt");
}

#[test]
fn test_bash_provenance_append_with_redirect() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "log.txt", "line one\n", "initial commit");

    pre_hook(&root, "append-sess", "append-t1");

    thread::sleep(Duration::from_millis(50));
    run_bash(&repo, "sh", &["-c", "echo 'appended line' >> log.txt"]);

    let post_action = post_hook(&root, "append-sess", "append-t1");
    assert_checkpoint_contains(&post_action, "log.txt");
}

#[test]
fn test_bash_provenance_truncate_to_zero() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(
        &repo,
        "data.txt",
        "lots of data here that will be erased",
        "initial commit",
    );

    pre_hook(&root, "trunc-sess", "trunc-t1");

    thread::sleep(Duration::from_millis(50));
    run_bash(&repo, "sh", &["-c", ": > data.txt"]);

    let post_action = post_hook(&root, "trunc-sess", "trunc-t1");
    assert_checkpoint_contains(&post_action, "data.txt");
}

#[cfg(unix)]
#[test]
fn test_bash_provenance_chmod_permission_change() {
    use git_ai::operations::commands::checkpoint_agent::bash_tool::diff;
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "script.sh", "#!/bin/bash\necho hi", "initial commit");

    let pre = snapshot(&root, "chmod-sess", "chmod-t1", None).unwrap();

    run_bash(&repo, "chmod", &["+x", "script.sh"]);

    let post = snapshot(&root, "chmod-sess", "chmod-t2", None).unwrap();
    let result = diff(&pre, &post);
    assert!(
        result
            .modified
            .iter()
            .any(|p| p.display().to_string().contains("script.sh")),
        "chmod should be detected via stat-tuple diff; got created={:?} modified={:?}",
        result.created,
        result.modified,
    );
}

#[test]
fn test_bash_provenance_mv_rename() {
    use git_ai::operations::commands::checkpoint_agent::bash_tool::diff;
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "old_name.txt", "rename me", "initial commit");

    let pre = snapshot(&root, "mv-sess", "mv-t1", None).unwrap();

    run_bash(&repo, "mv", &["old_name.txt", "new_name.txt"]);

    let post = snapshot(&root, "mv-sess", "mv-t2", None).unwrap();
    let result = diff(&pre, &post);
    assert!(
        result
            .created
            .iter()
            .any(|p| p.display().to_string().contains("new_name.txt")),
        "new_name.txt should appear as created after rename; got created={:?}",
        result.created,
    );
}

// ===========================================================================
// Category 5: Git commands (that modify working tree)
// ===========================================================================

#[test]
fn test_bash_provenance_git_checkout_restore() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(
        &repo,
        "restorable.txt",
        "original content",
        "initial commit",
    );

    // Modify the file so git checkout -- will revert it
    thread::sleep(Duration::from_millis(50));
    repo.write_file("restorable.txt", "modified content");

    pre_hook(&root, "checkout-sess", "checkout-t1");

    thread::sleep(Duration::from_millis(50));
    // Use git_og to bypass hooks, simulating what a bash command would do
    repo.git_og(&["checkout", "--", "restorable.txt"])
        .expect("git checkout should succeed");

    let post_action = post_hook(&root, "checkout-sess", "checkout-t1");
    assert_checkpoint_contains(&post_action, "restorable.txt");
}

#[test]
fn test_bash_provenance_git_stash_pop() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "stashed.txt", "original", "initial commit");

    // Modify and stash
    thread::sleep(Duration::from_millis(50));
    repo.write_file("stashed.txt", "modified for stash");
    repo.git_og(&["add", "stashed.txt"])
        .expect("git add should succeed");
    repo.git_og(&["stash", "push", "-m", "test stash"])
        .expect("git stash should succeed");

    pre_hook(&root, "stash-sess", "stash-t1");

    thread::sleep(Duration::from_millis(50));
    repo.git_og(&["stash", "pop"])
        .expect("git stash pop should succeed");

    let post_action = post_hook(&root, "stash-sess", "stash-t1");
    assert_checkpoint_contains(&post_action, "stashed.txt");
}

#[test]
fn test_bash_provenance_git_apply_patch() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(
        &repo,
        "patchme.txt",
        "line one\nline two\nline three\n",
        "initial",
    );

    // Create a patch file
    let patch_content = "\
--- a/patchme.txt
+++ b/patchme.txt
@@ -1,3 +1,3 @@
 line one
-line two
+line TWO PATCHED
 line three
";
    repo.write_file("fix.patch", patch_content);

    pre_hook(&root, "patch-sess", "patch-t1");

    thread::sleep(Duration::from_millis(50));
    repo.git_og(&["apply", "fix.patch"])
        .expect("git apply should succeed");

    let post_action = post_hook(&root, "patch-sess", "patch-t1");
    assert_checkpoint_contains(&post_action, "patchme.txt");
}

#[test]
fn test_bash_provenance_grep_sed_pipeline() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "file1.txt", "old pattern here", "add file1");
    add_and_commit(&repo, "file2.txt", "old pattern there", "add file2");
    add_and_commit(&repo, "file3.txt", "no match", "add file3");

    pre_hook(&root, "pipeline-sess", "pipeline-t1");

    thread::sleep(Duration::from_millis(50));
    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            "grep -rl 'old' --include='*.txt' . | xargs sed -i.bak 's/old/new/g' && find . -name '*.bak' -delete",
        ],
    );

    let post_action = post_hook(&root, "pipeline-sess", "pipeline-t1");
    assert_checkpoint_contains(&post_action, "file1.txt");
    assert_checkpoint_contains(&post_action, "file2.txt");
    assert_checkpoint_excludes(&post_action, "file3.txt");
}

#[test]
fn test_bash_provenance_touch_then_write_shows_created() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "touchwrite-sess", "touchwrite-t1");

    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            "touch empty.txt && echo 'now has content' > empty.txt",
        ],
    );

    let post_action = post_hook(&root, "touchwrite-sess", "touchwrite-t1");
    assert_checkpoint_contains(&post_action, "empty.txt");
}
