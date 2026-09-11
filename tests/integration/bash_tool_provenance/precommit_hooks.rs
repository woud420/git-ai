#[cfg(unix)]
use super::*;

#[cfg(unix)]
#[test]
fn test_bash_provenance_precommit_hook_formatter_modifies_staged_file() {
    // Scenario: AI agent creates a file, commits it, and the pre-commit hook
    // reformats the file (modifies it without re-staging). The stat-diff should
    // detect the formatter's modification.
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    // Install a pre-commit hook that appends a formatter comment to .py files.
    // It modifies the working tree file but does NOT re-stage, so the commit
    // contains the original content and the working tree has the formatted version.
    install_pre_commit_hook(
        &repo,
        r#"#!/bin/sh
for f in $(git diff --cached --name-only --diff-filter=ACM -- '*.py'); do
    echo '# auto-formatted' >> "$f"
done
exit 0
"#,
    );

    // Pre-snapshot: AI agent's bash tool starts
    pre_hook(&root, "fmt-sess", "fmt-t1");

    // AI agent creates and stages a Python file
    repo.write_file("main.py", "print('hello')\n");
    run_git_with_hooks(&repo, &["add", "main.py"]);

    // AI agent commits — the pre-commit hook will modify main.py
    let commit_output = run_git_with_hooks(&repo, &["commit", "-m", "add main.py"]);
    assert!(
        commit_output.status.success(),
        "git commit should succeed: {}",
        String::from_utf8_lossy(&commit_output.stderr)
    );

    // Verify the formatter actually ran
    let content = fs::read_to_string(repo.path().join("main.py")).unwrap();
    assert!(
        content.contains("# auto-formatted"),
        "pre-commit hook should have appended formatter comment; got: {:?}",
        content
    );

    // Post-snapshot: stat-diff should detect the formatter's modification
    let post_action = post_hook(&root, "fmt-sess", "fmt-t1");
    assert_checkpoint_contains(&post_action, "main.py");
}

#[cfg(unix)]
#[test]
fn test_bash_provenance_precommit_hook_formatter_restages_file() {
    // Scenario: pre-commit hook formats AND re-stages the file (common pattern
    // with tools like prettier --write + git add). The commit contains the
    // formatted content. The stat-diff should still detect the file change.
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    install_pre_commit_hook(
        &repo,
        r#"#!/bin/sh
for f in $(git diff --cached --name-only --diff-filter=ACM -- '*.py'); do
    # Simulate a formatter that normalizes whitespace
    sed -i.bak 's/[[:space:]]*$//' "$f" && rm -f "$f.bak"
    echo '# formatted-and-staged' >> "$f"
    git add "$f"
done
exit 0
"#,
    );

    pre_hook(&root, "fmtstage-sess", "fmtstage-t1");

    repo.write_file("app.py", "x = 1   \ny = 2   \n");
    run_git_with_hooks(&repo, &["add", "app.py"]);

    let commit_output = run_git_with_hooks(&repo, &["commit", "-m", "add app.py"]);
    assert!(
        commit_output.status.success(),
        "git commit should succeed: {}",
        String::from_utf8_lossy(&commit_output.stderr)
    );

    let content = fs::read_to_string(repo.path().join("app.py")).unwrap();
    assert!(
        content.contains("# formatted-and-staged"),
        "formatter should have modified the file; got: {:?}",
        content
    );

    let post_action = post_hook(&root, "fmtstage-sess", "fmtstage-t1");
    assert_checkpoint_contains(&post_action, "app.py");
}

#[cfg(unix)]
#[test]
fn test_bash_provenance_precommit_hook_creates_new_file() {
    // Scenario: pre-commit hook creates a new file (e.g., a lint report or
    // generated manifest). The stat-diff should detect the new file.
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    install_pre_commit_hook(
        &repo,
        r#"#!/bin/sh
# Generate a timestamp file on every commit
echo "last-commit: $(date -u +%Y-%m-%dT%H:%M:%S)" > .commit-metadata
exit 0
"#,
    );

    pre_hook(&root, "hooknew-sess", "hooknew-t1");

    repo.write_file("feature.py", "def feature(): pass\n");
    run_git_with_hooks(&repo, &["add", "feature.py"]);

    let commit_output = run_git_with_hooks(&repo, &["commit", "-m", "add feature"]);
    assert!(
        commit_output.status.success(),
        "git commit should succeed: {}",
        String::from_utf8_lossy(&commit_output.stderr)
    );

    // Verify the hook created the metadata file
    assert!(
        repo.path().join(".commit-metadata").exists(),
        "pre-commit hook should have created .commit-metadata"
    );

    let post_action = post_hook(&root, "hooknew-sess", "hooknew-t1");

    // Both the agent's file and the hook-created file should be detected
    assert_checkpoint_contains(&post_action, "feature.py");
    assert_checkpoint_contains(&post_action, ".commit-metadata");
}

#[cfg(unix)]
#[test]
fn test_bash_provenance_precommit_hook_modifies_untouched_file() {
    // Scenario: pre-commit hook modifies a file that the AI agent did NOT touch.
    // For example, a hook that updates a version timestamp in a config file
    // whenever any commit is made. The stat-diff should detect this change.
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");
    add_and_commit(&repo, "build-stamp.txt", "build: 0\n", "add build stamp");

    install_pre_commit_hook(
        &repo,
        r#"#!/bin/sh
# Increment build number on every commit (modifies a file the agent didn't touch)
current=$(grep -o '[0-9]*' build-stamp.txt)
next=$((current + 1))
echo "build: $next" > build-stamp.txt
exit 0
"#,
    );

    pre_hook(&root, "hookother-sess", "hookother-t1");

    // AI agent creates a completely different file
    thread::sleep(Duration::from_millis(50));
    repo.write_file("new-feature.rs", "fn new_feature() {}\n");
    run_git_with_hooks(&repo, &["add", "new-feature.rs"]);

    let commit_output = run_git_with_hooks(&repo, &["commit", "-m", "add new feature"]);
    assert!(
        commit_output.status.success(),
        "git commit should succeed: {}",
        String::from_utf8_lossy(&commit_output.stderr)
    );

    // Verify the hook modified build-stamp.txt
    let content = fs::read_to_string(repo.path().join("build-stamp.txt")).unwrap();
    assert!(
        content.contains("build: 1"),
        "hook should have incremented build number; got: {:?}",
        content
    );

    let post_action = post_hook(&root, "hookother-sess", "hookother-t1");

    // Both files should be detected: the agent's new file and the hook-modified file
    assert_checkpoint_contains(&post_action, "new-feature.rs");
    assert_checkpoint_contains(&post_action, "build-stamp.txt");
}

// test_bash_provenance_precommit_hook_with_agent_context_attribution was removed:
// checkpoint_context_from_active_bash has been deleted from the codebase.

#[cfg(unix)]
#[test]
fn test_bash_provenance_precommit_hook_modifies_multiple_files() {
    // Scenario: pre-commit hook runs a formatter on multiple staged files.
    // All modified files should be detected by the stat-diff.
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    install_pre_commit_hook(
        &repo,
        r#"#!/bin/sh
for f in $(git diff --cached --name-only --diff-filter=ACM -- '*.py'); do
    echo '# lint-pass' >> "$f"
done
exit 0
"#,
    );

    pre_hook(&root, "multi-fmt-sess", "multi-fmt-t1");

    // AI agent creates multiple Python files
    repo.write_file("src/api.py", "def api(): pass\n");
    repo.write_file("src/models.py", "class Model: pass\n");
    repo.write_file("src/utils.py", "def util(): pass\n");
    repo.write_file("readme.md", "# Project\n"); // Not .py — won't be formatted

    run_git_with_hooks(&repo, &["add", "."]);

    let commit_output = run_git_with_hooks(&repo, &["commit", "-m", "add src"]);
    assert!(
        commit_output.status.success(),
        "git commit should succeed: {}",
        String::from_utf8_lossy(&commit_output.stderr)
    );

    // Verify all .py files were formatted
    for py_file in &["src/api.py", "src/models.py", "src/utils.py"] {
        let content = fs::read_to_string(repo.path().join(py_file)).unwrap();
        assert!(
            content.contains("# lint-pass"),
            "{} should have been formatted; got: {:?}",
            py_file,
            content
        );
    }
    // readme.md should NOT have been formatted
    let readme = fs::read_to_string(repo.path().join("readme.md")).unwrap();
    assert!(
        !readme.contains("# lint-pass"),
        "readme.md should not have been formatted"
    );

    let post_action = post_hook(&root, "multi-fmt-sess", "multi-fmt-t1");

    // All created/modified files should be detected
    assert_checkpoint_contains(&post_action, "api.py");
    assert_checkpoint_contains(&post_action, "models.py");
    assert_checkpoint_contains(&post_action, "utils.py");
    assert_checkpoint_contains(&post_action, "readme.md");
}

#[cfg(unix)]
#[test]
fn test_bash_provenance_precommit_hook_fails_and_modifies_files() {
    // Scenario: pre-commit hook modifies files but then exits with non-zero
    // (e.g., a linter that fixes formatting but reports errors). The commit
    // fails, but the stat-diff should still detect the modified files because
    // the working tree was changed.
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    install_pre_commit_hook(
        &repo,
        r#"#!/bin/sh
# Fix formatting but report failure (like a strict linter)
for f in $(git diff --cached --name-only --diff-filter=ACM -- '*.py'); do
    echo '# auto-fixed' >> "$f"
done
exit 1
"#,
    );

    pre_hook(&root, "hookfail-sess", "hookfail-t1");

    repo.write_file("broken.py", "x=1\n");
    run_git_with_hooks(&repo, &["add", "broken.py"]);

    // The commit will FAIL because the hook exits 1
    let commit_output = run_git_with_hooks(&repo, &["commit", "-m", "try commit"]);
    assert!(
        !commit_output.status.success(),
        "git commit should fail due to hook exit 1"
    );

    // But the hook still modified the file
    let content = fs::read_to_string(repo.path().join("broken.py")).unwrap();
    assert!(
        content.contains("# auto-fixed"),
        "hook should have modified the file even though it failed; got: {:?}",
        content
    );

    // The stat-diff should detect the modification
    let post_action = post_hook(&root, "hookfail-sess", "hookfail-t1");
    assert_checkpoint_contains(&post_action, "broken.py");
}
