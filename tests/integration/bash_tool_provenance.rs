//! Integration tests for AI provenance tracking via bash tool pre/post snapshots.
//!
//! Each test simulates what happens when an AI coding agent executes a bash
//! command: the system takes a pre-snapshot of filesystem metadata, the bash
//! command runs, and then a post-snapshot detects which files changed. This
//! validates that the stat-diff mechanism correctly identifies created,
//! modified, and deleted files across a wide variety of real-world shell
//! commands.

use crate::bash_tool_common::{add_and_commit, post_hook, pre_hook, repo_root};
use crate::repos::test_repo::TestRepo;
#[cfg(unix)]
use crate::repos::write_executable_script;
use git_ai::operations::commands::checkpoint_agent::bash_tool::{
    BashCheckpointAction, BashPostHookResult, diff, git_status_fallback, snapshot,
};
#[cfg(unix)]
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run a bash command in the repo and assert it succeeds.
fn run_bash(repo: &TestRepo, program: &str, args: &[&str]) -> std::process::Output {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo.path())
        .output()
        .unwrap_or_else(|e| panic!("{} {:?} failed to start: {}", program, args, e));
    assert!(
        output.status.success(),
        "{} {:?} failed: {}",
        program,
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Assert that a BashCheckpointAction::Checkpoint contains the expected path.
fn assert_checkpoint_contains(result: &BashPostHookResult, expected_path: &str) {
    match &result.action {
        BashCheckpointAction::Checkpoint(paths) => {
            assert!(
                paths.iter().any(|p| p.contains(expected_path)),
                "Expected checkpoint to contain '{}'; got {:?}",
                expected_path,
                paths
            );
        }
        BashCheckpointAction::NoChanges => {
            panic!(
                "Expected Checkpoint containing '{}', got NoChanges",
                expected_path
            );
        }
        other => {
            panic!("Expected Checkpoint, got {:?}", other);
        }
    }
}

/// Assert that a BashCheckpointAction::Checkpoint does NOT contain a path.
fn assert_checkpoint_excludes(result: &BashPostHookResult, excluded_path: &str) {
    if let BashCheckpointAction::Checkpoint(paths) = &result.action {
        assert!(
            !paths.iter().any(|p| p.contains(excluded_path)),
            "Expected checkpoint NOT to contain '{}'; got {:?}",
            excluded_path,
            paths
        );
    }
}

/// Assert that a BashCheckpointAction is NoChanges.
fn assert_no_changes(result: &BashPostHookResult) {
    match &result.action {
        BashCheckpointAction::NoChanges => {}
        other => {
            panic!("Expected NoChanges, got {:?}", other);
        }
    }
}

/// Get the checkpoint paths from an action, panicking if not a Checkpoint.
fn checkpoint_paths(result: &BashPostHookResult) -> &[String] {
    match &result.action {
        BashCheckpointAction::Checkpoint(paths) => paths,
        other => panic!("Expected Checkpoint, got {:?}", other),
    }
}

// ===========================================================================
// Category 14: Pre-commit hook formatter attribution
//
// Verifies that when a git commit runs inside an AI agent's bash tool call,
// and git's pre-commit hook runs a formatter (or any tool that modifies files),
// those changes are properly detected by the stat-diff mechanism and attributed
// to the AI agent.
// ===========================================================================

/// Install a git pre-commit hook script in the test repo.
/// The hook must be executable and located at `.git/hooks/pre-commit`.
#[cfg(unix)]
fn install_pre_commit_hook(repo: &TestRepo, script: &str) {
    let git_dir = repo.path().join(".git");
    // For linked worktrees, .git is a file pointing to the real git dir
    let hooks_dir = if git_dir.is_file() {
        let content = fs::read_to_string(&git_dir).expect("read .git file");
        let real_git_dir = content
            .trim()
            .strip_prefix("gitdir: ")
            .expect("parse gitdir");
        std::path::PathBuf::from(real_git_dir).join("hooks")
    } else {
        git_dir.join("hooks")
    };
    fs::create_dir_all(&hooks_dir).expect("create hooks dir");
    let hook_path = hooks_dir.join("pre-commit");
    write_executable_script(&hook_path, script).expect("write pre-commit hook");
}

/// Run a raw git command in the repo (without bypassing hooks).
/// Unlike `run_bash`, this returns the full output including exit status
/// without asserting success, so we can check for hook failures.
#[cfg(unix)]
fn run_git_with_hooks(repo: &TestRepo, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {:?} failed to start: {}", args, e))
}

fn assert_read_only_command<T>(
    (session_id, tool_use_id): (&str, &str),
    prepare: impl FnOnce(&TestRepo),
    command: impl FnOnce(&TestRepo) -> T,
) {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    prepare(&repo);
    pre_hook(&root, session_id, tool_use_id);
    drop(command(&repo));
    let post_action = post_hook(&root, session_id, tool_use_id);
    assert_no_changes(&post_action);
}

mod bulk_changes;

mod precommit_hooks;

mod status_fallback;

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
