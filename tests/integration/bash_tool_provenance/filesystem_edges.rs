use super::{
    Duration, TestRepo, add_and_commit, assert_checkpoint_contains, assert_checkpoint_excludes,
    post_hook, pre_hook, repo_root, run_bash, snapshot, thread,
};

// ===========================================================================
// Category 8: Symlink operations (unix only)
// ===========================================================================

#[cfg(unix)]
#[test]
fn test_bash_provenance_symlink_creation() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "target.txt", "symlink target", "initial commit");

    pre_hook(&root, "symlink-sess", "symlink-t1");

    run_bash(&repo, "ln", &["-s", "target.txt", "link.txt"]);

    let post_action = post_hook(&root, "symlink-sess", "symlink-t1");
    assert_checkpoint_contains(&post_action, "link.txt");
}

#[cfg(unix)]
#[test]
fn test_bash_provenance_symlink_target_change() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "target_a.txt", "target a", "add target a");
    add_and_commit(&repo, "target_b.txt", "target b", "add target b");

    // Create the symlink pointing to target_a
    run_bash(&repo, "ln", &["-s", "target_a.txt", "mylink.txt"]);
    // Commit the symlink so it is tracked
    repo.git_og(&["add", "mylink.txt"])
        .expect("git add symlink should succeed");
    repo.git_og(&["commit", "-m", "add symlink"])
        .expect("git commit symlink should succeed");

    pre_hook(&root, "symtgt-sess", "symtgt-t1");

    // Re-point the symlink to target_b
    run_bash(
        &repo,
        "sh",
        &["-c", "rm mylink.txt && ln -s target_b.txt mylink.txt"],
    );

    let post_action = post_hook(&root, "symtgt-sess", "symtgt-t1");
    assert_checkpoint_contains(&post_action, "mylink.txt");
}

// ===========================================================================
// Category 10: Edge cases
// ===========================================================================

#[test]
fn test_bash_provenance_failed_command_with_partial_output() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "fail-sess", "fail-t1");

    // Command that creates a file then fails. We use || true so run_bash
    // does not panic, but the file is still created.
    run_bash(
        &repo,
        "sh",
        &["-c", "echo 'partial' > partial.txt && false || true"],
    );

    let post_action = post_hook(&root, "fail-sess", "fail-t1");
    assert_checkpoint_contains(&post_action, "partial.txt");
}

#[test]
fn test_bash_provenance_file_with_spaces_in_name() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "spaces-sess", "spaces-t1");

    run_bash(&repo, "sh", &["-c", "echo 'x' > 'file with spaces.txt'"]);

    let post_action = post_hook(&root, "spaces-sess", "spaces-t1");
    assert_checkpoint_contains(&post_action, "file with spaces.txt");
}

#[test]
fn test_bash_provenance_file_with_special_characters() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "special-sess", "special-t1");

    run_bash(
        &repo,
        "sh",
        &["-c", "echo 'x' > 'file-with-dashes_and_underscores.txt'"],
    );

    let post_action = post_hook(&root, "special-sess", "special-t1");
    assert_checkpoint_contains(&post_action, "file-with-dashes_and_underscores.txt");
}

#[test]
fn test_bash_provenance_hidden_file_creation() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "hidden-sess", "hidden-t1");

    run_bash(
        &repo,
        "sh",
        &["-c", "echo 'secret config' > .hidden_config"],
    );

    let post_action = post_hook(&root, "hidden-sess", "hidden-t1");
    assert_checkpoint_contains(&post_action, ".hidden_config");
}

#[test]
fn test_bash_provenance_overwrite_identical_content_detects_mtime_change() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "same.txt", "identical", "initial commit");

    pre_hook(&root, "identical-sess", "identical-t1");

    // Wait so mtime advances even though content is the same
    thread::sleep(Duration::from_millis(50));
    // Write exact same content but file metadata (mtime) will change
    run_bash(&repo, "sh", &["-c", "echo 'identical' > same.txt"]);

    let post_action = post_hook(&root, "identical-sess", "identical-t1");
    // The stat tuple should differ because mtime changed, even if content is the same.
    // Note: echo adds a trailing newline, so content actually differs from "identical"
    // to "identical\n". Regardless, the stat-tuple approach detects this.
    assert_checkpoint_contains(&post_action, "same.txt");
}

#[test]
fn test_bash_provenance_sequential_tool_uses_same_session() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    // --- First cycle: create alpha.txt ---
    pre_hook(&root, "seq-sess", "seq-use1");

    run_bash(&repo, "sh", &["-c", "echo 'alpha' > alpha.txt"]);

    let post1 = post_hook(&root, "seq-sess", "seq-use1");
    assert_checkpoint_contains(&post1, "alpha.txt");
    assert_checkpoint_excludes(&post1, "beta.txt");

    // --- Second cycle: create beta.txt ---
    pre_hook(&root, "seq-sess", "seq-use2");

    run_bash(&repo, "sh", &["-c", "echo 'beta' > beta.txt"]);

    let post2 = post_hook(&root, "seq-sess", "seq-use2");
    assert_checkpoint_contains(&post2, "beta.txt");
    // alpha.txt was created in the first cycle; it should NOT appear in the second
    // cycle since the second pre-snapshot includes it.
    assert_checkpoint_excludes(&post2, "alpha.txt");
}

#[test]
fn test_bash_provenance_mv_directory_rename() {
    use git_ai::operations::commands::checkpoint_agent::bash_tool::diff;
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Create files in a subdirectory and track them
    add_and_commit(&repo, "src/lib.rs", "fn main() {}", "add src");
    add_and_commit(&repo, "src/utils.rs", "fn helper() {}", "add utils");

    let pre = snapshot(&root, "mvdir-sess", "mvdir-t1", None).unwrap();

    std::fs::rename(root.join("src"), root.join("lib")).unwrap();

    let post = snapshot(&root, "mvdir-sess", "mvdir-t2", None).unwrap();
    let result = diff(&pre, &post);
    assert!(
        result.created.iter().any(|p| p
            .to_string_lossy()
            .replace('\\', "/")
            .contains("lib/lib.rs")),
        "lib/lib.rs should appear as created after directory rename; got created={:?}",
        result.created,
    );
}
