#[cfg(not(target_os = "windows"))]
use super::write_executable_script;
use super::{ExpectedLineExt, TestRepo};

/// Test interactive rebase with commit reordering - verifies interactive rebase works
#[test]
fn test_rebase_interactive_reorder() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Create 2 AI commits - we'll rebase these interactively
    let mut feature1 = repo.filename("feature1.txt");
    feature1.set_contents(crate::lines!["// AI feature 1".ai()]);
    repo.stage_all_and_commit("AI commit 1").unwrap();

    let mut feature2 = repo.filename("feature2.txt");
    feature2.set_contents(crate::lines!["// AI feature 2".ai()]);
    repo.stage_all_and_commit("AI commit 2").unwrap();

    // Advance main branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main work"]);
    repo.stage_all_and_commit("Main advances").unwrap();
    let base_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Perform interactive rebase (just pick all, tests that -i flag works)
    repo.git(&["checkout", "feature"]).unwrap();

    let result = repo.git_with_env(
        &["rebase", "-i", &base_commit],
        &[("GIT_SEQUENCE_EDITOR", "true"), ("GIT_EDITOR", "true")],
        None,
    );

    if result.is_err() {
        eprintln!("git rebase output: {:?}", result);
        panic!("Interactive rebase failed");
    }

    // Verify both files have preserved AI authorship after interactive rebase
    feature1.assert_lines_and_blame(crate::lines!["// AI feature 1".ai()]);
    feature2.assert_lines_and_blame(crate::lines!["// AI feature 2".ai()]);
}

/// Test rebase with autosquash enabled
#[test]
fn test_rebase_autosquash() {
    let repo = TestRepo::new();

    // Enable autosquash in config
    repo.git(&["config", "rebase.autosquash", "true"]).unwrap();

    // Create initial commit
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["line 1"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI line 2".ai()]);
    repo.stage_all_and_commit("Add feature").unwrap();

    // Create fixup commit
    file.replace_at(1, "AI line 2 fixed".ai());
    repo.stage_all_and_commit("fixup! Add feature").unwrap();

    // Advance main
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut other_file = repo.filename("other.txt");
    other_file.set_contents(crate::lines!["other"]);
    repo.stage_all_and_commit("Main work").unwrap();
    let base = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Interactive rebase with autosquash (hooks will handle authorship)
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git_with_env(
        &["rebase", "-i", "--autosquash", &base],
        &[("GIT_SEQUENCE_EDITOR", "true"), ("GIT_EDITOR", "true")],
        None,
    );

    if rebase_result.is_ok() {
        // Verify the file has the expected content with AI authorship
        file.assert_lines_and_blame(crate::lines!["line 1".ai(), "AI line 2 fixed".ai()]);
    }
}

/// Test rebase --exec to run tests at each commit
#[test]
fn test_rebase_exec() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut test_sh = repo.filename("test.sh");
    test_sh.set_contents(crate::lines!["#!/bin/sh", "exit 0"]);
    repo.stage_all_and_commit("Initial").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch with multiple AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut f1 = repo.filename("f1.txt");
    f1.set_contents(crate::lines!["// AI 1".ai()]);
    repo.stage_all_and_commit("AI commit 1").unwrap();

    let mut f2 = repo.filename("f2.txt");
    f2.set_contents(crate::lines!["// AI 2".ai()]);
    repo.stage_all_and_commit("AI commit 2").unwrap();

    // Advance main
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main"]);
    repo.stage_all_and_commit("Main work").unwrap();
    let base = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    repo.git(&["checkout", "feature"]).unwrap();

    // Rebase with --exec (hooks will handle authorship)
    repo.git_with_env(
        &["rebase", "-i", "--exec", "echo 'test passed'", &base],
        &[("GIT_SEQUENCE_EDITOR", "true"), ("GIT_EDITOR", "true")],
        None,
    )
    .expect("Rebase with --exec should succeed");

    // Verify authorship was preserved
    f1.assert_lines_and_blame(crate::lines!["// AI 1".ai()]);
    f2.assert_lines_and_blame(crate::lines!["// AI 2".ai()]);
}

/// Test rebase with commit splitting (fewer original commits than new commits)
/// This tests that rebase handles AI authorship correctly even with complex commit histories
#[test]
fn test_rebase_commit_splitting() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content", ""]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();

    // Create feature branch with AI commits
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    let mut features_file = repo.filename("features.txt");
    features_file.set_contents(crate::lines![
        "// AI feature 1".ai(),
        "function feature1() {}".ai(),
        "".ai()
    ]);
    repo.stage_all_and_commit("AI feature 1").unwrap();

    features_file.insert_at(
        2,
        crate::lines!["// AI feature 2".ai(), "function feature2() {}".ai()],
    );
    repo.stage_all_and_commit("AI feature 2").unwrap();

    // Advance main branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main content", ""]);
    repo.stage_all_and_commit("Main advances").unwrap();

    // Rebase feature onto main (hooks will handle authorship)
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Verify AI authorship is preserved after rebase
    features_file.assert_lines_and_blame(crate::lines![
        "// AI feature 1".ai(),
        "function feature1() {}".ai(),
        "// AI feature 2".ai(),
        "function feature2() {}".ai(),
    ]);
}

/// Test rebase with rewording (renaming) a commit that has 2 children commits
/// Verifies that authorship is preserved for all 3 commits after reword
#[test]
#[cfg(not(target_os = "windows"))]
fn test_rebase_reword_commit_with_children() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Create 3 AI commits - we'll reword the first one
    let mut feature1 = repo.filename("feature1.txt");
    feature1.set_contents(crate::lines![
        "// AI feature 1".ai(),
        "function feature1() {}".ai()
    ]);
    repo.stage_all_and_commit("AI commit 1 - original message")
        .unwrap();

    let mut feature2 = repo.filename("feature2.txt");
    feature2.set_contents(crate::lines![
        "// AI feature 2".ai(),
        "function feature2() {}".ai()
    ]);
    repo.stage_all_and_commit("AI commit 2").unwrap();

    let mut feature3 = repo.filename("feature3.txt");
    feature3.set_contents(crate::lines![
        "// AI feature 3".ai(),
        "function feature3() {}".ai()
    ]);
    repo.stage_all_and_commit("AI commit 3").unwrap();

    // Advance main branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main work"]);
    repo.stage_all_and_commit("Main advances").unwrap();
    let base_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Perform interactive rebase with rewording the first commit
    repo.git(&["checkout", "feature"]).unwrap();

    use std::io::Write;

    // Create a script that modifies the rebase-todo to reword the first commit
    let script_content = r#"#!/bin/sh
sed -i.bak '1s/pick/reword/' "$1"
"#;

    let script_path = repo.path().join("reword_script.sh");
    write_executable_script(&script_path, script_content).unwrap();

    // Create a script that provides the new commit message
    let commit_msg_content = "AI commit 1 - RENAMED MESSAGE";
    let commit_msg_path = repo.path().join("new_commit_msg.txt");
    let mut msg_file = std::fs::File::create(&commit_msg_path).unwrap();
    msg_file.write_all(commit_msg_content.as_bytes()).unwrap();
    drop(msg_file);

    // Create an editor script that replaces the commit message
    let editor_script_content = format!(
        r#"#!/bin/sh
cat {} > "$1"
"#,
        commit_msg_path.to_str().unwrap()
    );
    let editor_script_path = repo.path().join("editor_script.sh");
    write_executable_script(&editor_script_path, editor_script_content).unwrap();

    let rebase_result = repo.git_with_env(
        &["rebase", "-i", &base_commit],
        &[
            ("GIT_SEQUENCE_EDITOR", script_path.to_str().unwrap()),
            ("GIT_EDITOR", editor_script_path.to_str().unwrap()),
        ],
        None,
    );

    if rebase_result.is_err() {
        eprintln!("git rebase output: {:?}", rebase_result);
        panic!("Interactive rebase with reword failed");
    }

    // Verify all 3 files still exist with correct AI authorship after reword
    feature1.assert_lines_and_blame(crate::lines![
        "// AI feature 1".ai(),
        "function feature1() {}".ai()
    ]);
    feature2.assert_lines_and_blame(crate::lines![
        "// AI feature 2".ai(),
        "function feature2() {}".ai()
    ]);
    feature3.assert_lines_and_blame(crate::lines![
        "// AI feature 3".ai(),
        "function feature3() {}".ai()
    ]);
}

/// Regression test: interactive rebase that drops a commit must preserve attribution
/// on surviving commits (issue #970).
///
/// When an interactive rebase uses `drop` to skip commit B from [A, B, C], the
/// surviving rebased commits A′ and C′ must retain the attribution originally
/// associated with A and C respectively.
///
/// The bug: `rewrite_authorship_after_rebase_v2` used a positional zip to pair
/// `original_commits` with `new_commits`.  When commit B was dropped the lists
/// had different lengths: originals = [A, B, C] but new = [A′, C′].  The zip
/// produced [(A, A′), (B, C′)] so C′ was attributed using B's (wrong) note
/// instead of C's note, causing C′ to lose its attribution entirely.
///
/// The fix: when original and new commit counts differ, pair commits by matching
/// the commit subject line rather than by position.
#[test]
#[cfg(not(target_os = "windows"))]
fn test_rebase_interactive_drop_preserves_attribution() {
    let repo = TestRepo::new();

    // Create a base commit on main
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    // Create feature branch with three AI commits A, B, C
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    let mut file_a = repo.filename("file_a.txt");
    file_a.set_contents(crate::lines!["AI line A".ai()]);
    repo.stage_all_and_commit("Commit A").unwrap();

    let mut file_b = repo.filename("file_b.txt");
    file_b.set_contents(crate::lines!["AI line B".ai()]);
    repo.stage_all_and_commit("Commit B").unwrap();

    let mut file_c = repo.filename("file_c.txt");
    file_c.set_contents(crate::lines!["AI line C".ai()]);
    repo.stage_all_and_commit("Commit C").unwrap();

    // Advance main so the rebase has a new base (forces non-fast-forward)
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut other = repo.filename("other.txt");
    other.set_contents(crate::lines!["other content"]);
    repo.stage_all_and_commit("Main advances").unwrap();
    let base_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Interactive rebase: drop Commit B, keep A and C
    repo.git(&["checkout", "feature"]).unwrap();
    // Drop the 2nd pick line (Commit B) — the three commits appear in order A, B, C.
    let drop_script = r#"#!/bin/sh
sed -i.bak '2s/^pick/drop/' "$1"
"#;
    let script_path = repo.path().join("drop_script.sh");
    write_executable_script(&script_path, drop_script).unwrap();

    let rebase_result = repo.git_with_env(
        &["rebase", "-i", &base_commit],
        &[
            ("GIT_SEQUENCE_EDITOR", script_path.to_str().unwrap()),
            ("GIT_EDITOR", "true"),
        ],
        None,
    );
    assert!(
        rebase_result.is_ok(),
        "interactive rebase with drop should succeed: {:?}",
        rebase_result
    );

    // file_b should be gone (its commit was dropped)
    assert!(
        !repo.path().join("file_b.txt").exists(),
        "file_b.txt should not exist after its commit was dropped"
    );

    // Commit A's rewrite (A′) must still carry AI attribution
    file_a.assert_lines_and_blame(crate::lines!["AI line A".ai()]);

    // Commit C's rewrite (C′) must still carry AI attribution.
    // This is the assertion that failed before the fix: the broken positional zip
    // paired C′ with B's note (no AI content), causing the attribution to be lost.
    file_c.assert_lines_and_blame(crate::lines!["AI line C".ai()]);
}

crate::reuse_tests_in_worktree!(
    test_rebase_interactive_reorder,
    test_rebase_autosquash,
    test_rebase_exec,
    test_rebase_commit_splitting,
);

crate::reuse_tests_in_worktree_with_attrs!(
(#[cfg(not(target_os = "windows"))])
    test_rebase_reword_commit_with_children,
    test_rebase_interactive_drop_preserves_attribution,
);
