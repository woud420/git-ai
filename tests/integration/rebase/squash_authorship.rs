#[cfg(not(target_os = "windows"))]
use super::{ExpectedLineExt, TestRepo, write_executable_script};

/// Test interactive rebase with squashing - verifies authorship from all commits is preserved
/// This tests that squashing preserves authorship from all commits
#[test]
#[cfg(not(target_os = "windows"))]
fn test_rebase_squash_preserves_all_authorship() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Create 3 AI commits with different content - we'll squash these
    let mut feature1 = repo.filename("feature1.txt");
    feature1.set_contents(crate::lines!["// AI feature 1".ai(), "line 1".ai()]);
    repo.stage_all_and_commit("AI commit 1").unwrap();

    let mut feature2 = repo.filename("feature2.txt");
    feature2.set_contents(crate::lines!["// AI feature 2".ai(), "line 2".ai()]);
    repo.stage_all_and_commit("AI commit 2").unwrap();

    let mut feature3 = repo.filename("feature3.txt");
    feature3.set_contents(crate::lines!["// AI feature 3".ai(), "line 3".ai()]);
    repo.stage_all_and_commit("AI commit 3").unwrap();

    // Advance main branch
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file = repo.filename("main.txt");
    main_file.set_contents(crate::lines!["main work"]);
    repo.stage_all_and_commit("Main advances").unwrap();
    let base_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Perform interactive rebase with squashing: pick first, squash second and third
    repo.git(&["checkout", "feature"]).unwrap();

    // Create a script that modifies the rebase-todo to squash commits 2 and 3 into 1
    let script_content = r#"#!/bin/sh
sed -i.bak '2s/pick/squash/' "$1"
sed -i.bak '3s/pick/squash/' "$1"
"#;

    let script_path = repo.path().join("squash_script.sh");
    write_executable_script(&script_path, script_content).unwrap();

    let rebase_result = repo.git_with_env(
        &["rebase", "-i", &base_commit],
        &[
            ("GIT_SEQUENCE_EDITOR", script_path.to_str().unwrap()),
            ("GIT_EDITOR", "true"),
        ],
        None,
    );

    if rebase_result.is_err() {
        eprintln!("git rebase output: {:?}", rebase_result);
        panic!("Interactive rebase with squash failed");
    }

    // Verify all 3 files exist with preserved AI authorship after squashing
    assert!(
        repo.path().join("feature1.txt").exists(),
        "feature1.txt from commit 1 should exist"
    );
    assert!(
        repo.path().join("feature2.txt").exists(),
        "feature2.txt from commit 2 should exist"
    );
    assert!(
        repo.path().join("feature3.txt").exists(),
        "feature3.txt from commit 3 should exist"
    );

    // Verify AI authorship was preserved through squashing
    feature1.assert_lines_and_blame(crate::lines!["// AI feature 1".ai(), "line 1".ai()]);
    feature2.assert_lines_and_blame(crate::lines!["// AI feature 2".ai(), "line 2".ai()]);
    feature3.assert_lines_and_blame(crate::lines!["// AI feature 3".ai(), "line 3".ai()]);
}

/// Regression test for issue #1214: after squash rebase of 3 commits (2 AI + 1 human),
/// the merged note loses the humans block entirely — known-human line attribution is gone.
///
/// Repro from the issue:
/// 1. AI commit 1: AI adds lines to a file (with some lines the human later overrides)
/// 2. Human commit: human edits the same file (known_human checkpoint)
/// 3. AI commit 2: AI adds more lines
/// 4. git rebase -i HEAD~3 → fixup all into first commit
/// 5. Inspect merged note → humans block must be preserved
#[test]
#[cfg(not(target_os = "windows"))]
fn test_rebase_squash_preserves_human_attribution() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    let file_path = repo.path().join("handler.go");

    // --- AI commit 1: AI adds lines ---
    // Pre-edit checkpoint: file doesn't exist yet, take a snapshot of "nothing"
    repo.git_ai(&["checkpoint", "human", "handler.go"]).unwrap();
    let ai_content_1 = "\
func handleOrder() {
    validate()
    process()
}
";
    std::fs::write(&file_path, ai_content_1).unwrap();
    // Post-edit checkpoint: AI wrote the content
    repo.git_ai(&["checkpoint", "mock_ai", "handler.go"])
        .unwrap();
    repo.stage_all_and_commit("AI commit 1").unwrap();

    let mut handler = repo.filename("handler.go");
    handler.assert_committed_lines(crate::lines![
        "func handleOrder() {".ai(),
        "    validate()".ai(),
        "    process()".ai(),
        "}".ai(),
    ]);

    // --- Human commit: human edits the file, adding a line ---
    let human_content = "\
func handleOrder() {
    validate()
    log(\"order received\")
    process()
}
";
    std::fs::write(&file_path, human_content).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "handler.go"])
        .unwrap();
    repo.stage_all_and_commit("Human commit").unwrap();

    handler.assert_committed_lines(crate::lines![
        "func handleOrder() {".ai(),
        "    validate()".ai(),
        "    log(\"order received\")".human(),
        "    process()".ai(),
        "}".ai(),
    ]);

    // Verify humans block exists before squash
    let human_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let human_log = repo.require_authorship_log(&human_sha);
    assert!(
        !human_log.metadata.humans.is_empty(),
        "Pre-squash: human commit should have humans metadata block"
    );

    // --- AI commit 2: AI adds more lines ---
    // Pre-edit checkpoint: snapshot current state before AI edits
    repo.git_ai(&["checkpoint", "human", "handler.go"]).unwrap();
    let ai_content_2 = "\
func handleOrder() {
    validate()
    log(\"order received\")
    process()
    sendMetrics()
}
";
    std::fs::write(&file_path, ai_content_2).unwrap();
    // Post-edit checkpoint: AI wrote the new line
    repo.git_ai(&["checkpoint", "mock_ai", "handler.go"])
        .unwrap();
    repo.stage_all_and_commit("AI commit 2").unwrap();

    handler.assert_committed_lines(crate::lines![
        "func handleOrder() {".ai(),
        "    validate()".ai(),
        "    log(\"order received\")".human(),
        "    process()".ai(),
        "    sendMetrics()".ai(),
        "}".ai(),
    ]);

    // Advance main branch so rebase has something to replay onto
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file2 = repo.filename("main2.txt");
    main_file2.set_contents(crate::lines!["main work"]);
    repo.stage_all_and_commit("Main advances").unwrap();
    let base_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // --- Squash rebase: fixup all 3 commits into the first ---
    repo.git(&["checkout", "feature"]).unwrap();

    let script_content = r#"#!/bin/sh
sed -i.bak '2s/pick/fixup/' "$1"
sed -i.bak '3s/pick/fixup/' "$1"
"#;

    let script_path = repo.path().join("squash_script.sh");
    write_executable_script(&script_path, script_content).unwrap();

    let rebase_result = repo.git_with_env(
        &["rebase", "-i", &base_commit],
        &[
            ("GIT_SEQUENCE_EDITOR", script_path.to_str().unwrap()),
            ("GIT_EDITOR", "true"),
        ],
        None,
    );

    if rebase_result.is_err() {
        eprintln!("git rebase output: {:?}", rebase_result);
        panic!("Interactive rebase with fixup failed");
    }

    // Verify file content survived the squash
    assert!(
        repo.path().join("handler.go").exists(),
        "handler.go should exist after squash"
    );

    // Verify the merged note has the humans block preserved
    let squashed_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let squashed_log = repo.require_authorship_log(&squashed_sha);
    assert!(
        !squashed_log.metadata.humans.is_empty(),
        "Post-squash: humans metadata block must be preserved (issue #1214)"
    );
    for record in squashed_log.metadata.humans.values() {
        assert_eq!(
            record.author, "Test User <test@example.com>",
            "HumanRecord.author should include email"
        );
    }

    // Verify line-level attribution: human line must still show as human,
    // and AI lines (including closing `}`) retain their attribution through squash.
    handler.assert_lines_and_blame(crate::lines![
        "func handleOrder() {".ai(),
        "    validate()".ai(),
        "    log(\"order received\")".human(),
        "    process()".ai(),
        "    sendMetrics()".ai(),
        "}".ai(),
    ]);
}

/// Verify that session metadata survives squash rebase.
/// This is the session-format counterpart of test_rebase_squash_preserves_human_attribution.
/// Sessions use s_<id>::t_<hash> attestation entries and are the current default format.
/// The delta_sessions code already scans current_attributions (unlike the old delta_humans
/// code), so this test should pass without additional fixes.
#[test]
#[cfg(not(target_os = "windows"))]
fn test_rebase_squash_preserves_session_attribution() {
    let repo = TestRepo::new();

    // Create initial commit on default branch
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let default_branch = repo.current_branch();
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    let file_path = repo.path().join("service.go");

    // --- AI commit 1: AI adds initial lines ---
    repo.git_ai(&["checkpoint", "human", "service.go"]).unwrap();
    let ai_content_1 = "\
func serve() {
    listen()
    handle()
}
";
    std::fs::write(&file_path, ai_content_1).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "service.go"])
        .unwrap();
    repo.stage_all_and_commit("AI commit 1").unwrap();

    let mut service = repo.filename("service.go");
    service.assert_committed_lines(crate::lines![
        "func serve() {".ai(),
        "    listen()".ai(),
        "    handle()".ai(),
        "}".ai(),
    ]);

    // Verify session metadata exists on commit 1
    let sha1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log1 = repo.require_authorship_log(&sha1);
    assert_eq!(
        log1.metadata.sessions.len(),
        1,
        "AI commit 1 should have exactly 1 session"
    );

    // --- AI commit 2: AI adds more lines ---
    repo.git_ai(&["checkpoint", "human", "service.go"]).unwrap();
    let ai_content_2 = "\
func serve() {
    listen()
    handle()
    logMetrics()
}
";
    std::fs::write(&file_path, ai_content_2).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "service.go"])
        .unwrap();
    repo.stage_all_and_commit("AI commit 2").unwrap();

    service.assert_committed_lines(crate::lines![
        "func serve() {".ai(),
        "    listen()".ai(),
        "    handle()".ai(),
        "    logMetrics()".ai(),
        "}".ai(),
    ]);

    // --- AI commit 3: AI adds yet more ---
    repo.git_ai(&["checkpoint", "human", "service.go"]).unwrap();
    let ai_content_3 = "\
func serve() {
    listen()
    handle()
    logMetrics()
    shutdown()
}
";
    std::fs::write(&file_path, ai_content_3).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "service.go"])
        .unwrap();
    repo.stage_all_and_commit("AI commit 3").unwrap();

    service.assert_committed_lines(crate::lines![
        "func serve() {".ai(),
        "    listen()".ai(),
        "    handle()".ai(),
        "    logMetrics()".ai(),
        "    shutdown()".ai(),
        "}".ai(),
    ]);

    // Advance main branch so rebase has something to replay onto
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_file2 = repo.filename("main2.txt");
    main_file2.set_contents(crate::lines!["main work"]);
    repo.stage_all_and_commit("Main advances").unwrap();
    let base_commit = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // --- Squash rebase: fixup all 3 commits into the first ---
    repo.git(&["checkout", "feature"]).unwrap();

    let script_content = r#"#!/bin/sh
sed -i.bak '2s/pick/fixup/' "$1"
sed -i.bak '3s/pick/fixup/' "$1"
"#;

    let script_path = repo.path().join("squash_script.sh");
    write_executable_script(&script_path, script_content).unwrap();

    let rebase_result = repo.git_with_env(
        &["rebase", "-i", &base_commit],
        &[
            ("GIT_SEQUENCE_EDITOR", script_path.to_str().unwrap()),
            ("GIT_EDITOR", "true"),
        ],
        None,
    );

    if rebase_result.is_err() {
        eprintln!("git rebase output: {:?}", rebase_result);
        panic!("Interactive rebase with fixup failed");
    }

    // Verify the merged note has sessions metadata preserved
    let squashed_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let squashed_log = repo.require_authorship_log(&squashed_sha);
    // Each mock_ai checkpoint creates a distinct session, so the squashed
    // note should have all 3 sessions merged from the 3 original commits.
    assert_eq!(
        squashed_log.metadata.sessions.len(),
        3,
        "Post-squash: squashed note should have all 3 sessions merged"
    );

    // Verify line-level AI attribution survived the squash
    service.assert_lines_and_blame(crate::lines![
        "func serve() {".ai(),
        "    listen()".ai(),
        "    handle()".ai(),
        "    logMetrics()".ai(),
        "    shutdown()".ai(),
        "}".ai(),
    ]);
}

crate::reuse_tests_in_worktree_with_attrs!(
(#[cfg(not(target_os = "windows"))])
    test_rebase_squash_preserves_all_authorship,
    test_rebase_squash_preserves_human_attribution,
    test_rebase_squash_preserves_session_attribution,
);
