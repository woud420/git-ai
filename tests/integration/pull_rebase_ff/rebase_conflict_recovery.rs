use super::{
    AuthorshipLog, ExpectedLineExt, TestRepo, checkpoint_claude_file_edit,
    setup_regular_rebase_conflict,
};

#[test]
fn test_regular_rebase_with_conflict_preserves_ai_notes() {
    let setup = setup_regular_rebase_conflict();
    let repo = setup.repo;

    // Verify AI commit has authorship notes before rebase
    let pre_rebase_note = repo.read_authorship_note(&setup.feature_ai_commit_sha);
    assert!(
        pre_rebase_note.is_some(),
        "Feature AI commit should have authorship notes before rebase"
    );
    let pre_rebase_note = pre_rebase_note.unwrap();
    let pre_rebase_log =
        AuthorshipLog::deserialize_from_string(&pre_rebase_note).expect("parse pre-rebase note");
    assert!(
        !pre_rebase_log.metadata.sessions.is_empty(),
        "precondition: feature AI commit should have session metadata"
    );

    // Rebase feature onto main — should conflict on shared.txt
    let rebase_result = repo.git(&["rebase", &setup.default_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should fail due to conflict on shared.txt"
    );

    // Resolve the conflict: keep both changes
    use std::fs;
    fs::write(
        repo.path().join("shared.txt"),
        "line 1\nmain change line 2\nAI feature line 2\n",
    )
    .expect("writing resolved file should succeed");

    repo.git(&["add", "shared.txt"])
        .expect("staging resolved file should succeed");

    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should succeed");

    // The rebased commit has a new SHA
    let new_head = repo
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    assert_ne!(
        new_head, setup.feature_ai_commit_sha,
        "HEAD should have a new SHA after rebase"
    );

    // Verify authorship notes were preserved on the new commit
    let post_rebase_note = repo.read_authorship_note(&new_head);
    assert!(
        post_rebase_note.is_some(),
        "Rebased commit should have authorship notes (notes should follow SHA rewrite)"
    );

    // After conflict resolution, AI-attributed lines fall inside diff hunks
    // (git diff-tree shows the region as modified), so attribution is correctly dropped.
    // The note exists (metadata preserved) but shared.txt has no attributed lines.
    let note_content = post_rebase_note.unwrap();
    let post_rebase_log =
        AuthorshipLog::deserialize_from_string(&note_content).expect("parse post-rebase note");
    assert_eq!(
        post_rebase_log.metadata.sessions, pre_rebase_log.metadata.sessions,
        "session metadata should be preserved even when changed-hunk attestations are dropped"
    );
    assert!(
        !note_content.contains("shared.txt"),
        "Authorship note should NOT reference shared.txt (lines inside diff hunk), got: {}",
        note_content
    );
}

#[test]
fn test_regular_rebase_two_conflicts_ai_rewrite_after_skipped_conflict_is_attributed() {
    use std::fs;

    let repo = TestRepo::new();
    let jokes_path = repo.path().join("jokes-programming.csv");
    let base = "\
setup,punchline
How many programmers does it take to change a light bulb?,None that's a hardware problem
Why do Java developers wear glasses?,Because they don't C#
Why did the programmer quit his job?,Because he didn't get arrays
";

    fs::write(&jokes_path, base).expect("write base jokes");
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-programming.csv"])
        .expect("base AI checkpoint should succeed");
    repo.stage_all_and_commit("Base jokes")
        .expect("base commit should succeed");
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "scenario-2-multi-conflict-same-file"])
        .expect("checkout feature branch should succeed");
    fs::write(
        &jokes_path,
        format!(
            "{}Why do Python developers make bad partners?,They only speak one language\n",
            base
        ),
    )
    .expect("write first feature joke");
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-programming.csv"])
        .expect("first feature AI checkpoint should succeed");
    repo.stage_all_and_commit("Add Python joke")
        .expect("first feature commit should succeed");

    fs::write(
        &jokes_path,
        format!(
            "{}Why do Python developers make bad partners?,They only speak one language\nHow many Rust developers does it take to change a lightbulb?,Two one to change it and one to write a song about the old one\n",
            base
        ),
    )
    .expect("write second feature joke");
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-programming.csv"])
        .expect("second feature AI checkpoint should succeed");
    repo.stage_all_and_commit("Add Rust joke")
        .expect("second feature commit should succeed");

    repo.git(&["checkout", &default_branch])
        .expect("checkout default branch should succeed");
    let main = format!(
        "{}Why do C++ developers get halloween mixed up with christmas?,Because Oct31 equals Dec25\nWhy did the developer go broke?,Because he used up all his cache\n",
        base
    );
    fs::write(&jokes_path, &main).expect("write main jokes");
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-programming.csv"])
        .expect("main AI checkpoint should succeed");
    repo.stage_all_and_commit("Add C++ jokes")
        .expect("main commit should succeed");

    let rebase_result = repo.git(&[
        "rebase",
        &default_branch,
        "scenario-2-multi-conflict-same-file",
    ]);
    assert!(
        rebase_result.is_err(),
        "first rebase stop should conflict on the Python joke"
    );

    checkpoint_claude_file_edit(
        &repo,
        "PreToolUse",
        "jokes-programming.csv",
        "resolve-first",
    );
    fs::write(&jokes_path, &main).expect("resolve first conflict by keeping main side");
    checkpoint_claude_file_edit(
        &repo,
        "PostToolUse",
        "jokes-programming.csv",
        "resolve-first",
    );
    repo.git(&["add", "jokes-programming.csv"])
        .expect("stage first conflict resolution");
    let second_stop = repo.git(&["rebase", "--skip"]);
    assert!(
        second_stop.is_err(),
        "skipping the first feature commit should immediately stop on the Rust conflict"
    );

    let rewritten = format!(
        "{}Why do C++ developers get halloween mixed up with christmas?,Because Oct31 equals Dec25\nWhy did the developer go broke?,Because he used up all his cache\nWhy do Rust developers write songs?,Because they're afraid of memory leaks in the lyrics\n",
        base
    );
    repo.git_ai(&["checkpoint", "human", "jokes-programming.csv"])
        .expect("pre-resolution checkpoint should succeed");
    fs::write(&jokes_path, rewritten).expect("rewrite second conflict resolution");
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-programming.csv"])
        .expect("AI resolution checkpoint should succeed");
    repo.git(&["add", "jokes-programming.csv"])
        .expect("stage AI conflict resolution");
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should finish");

    let mut jokes = repo.filename("jokes-programming.csv");
    jokes.assert_committed_lines(crate::lines![
        "setup,punchline".ai(),
        "How many programmers does it take to change a light bulb?,None that's a hardware problem"
            .ai(),
        "Why do Java developers wear glasses?,Because they don't C#".ai(),
        "Why did the programmer quit his job?,Because he didn't get arrays".ai(),
        "Why do C++ developers get halloween mixed up with christmas?,Because Oct31 equals Dec25"
            .ai(),
        "Why did the developer go broke?,Because he used up all his cache".ai(),
        "Why do Rust developers write songs?,Because they're afraid of memory leaks in the lyrics"
            .ai(),
    ]);
}

#[test]
fn test_regular_rebase_two_conflicts_ai_rewrite_after_empty_continue_is_attributed() {
    use std::fs;

    let repo = TestRepo::new();
    let jokes_path = repo.path().join("jokes-programming.csv");
    let base = "\
setup,punchline
How many programmers does it take to change a light bulb?,None that's a hardware problem
Why do Java developers wear glasses?,Because they don't C#
Why did the programmer quit his job?,Because he didn't get arrays
";

    fs::write(&jokes_path, base).expect("write base jokes");
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-programming.csv"])
        .expect("base AI checkpoint should succeed");
    repo.stage_all_and_commit("Base jokes")
        .expect("base commit should succeed");
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "scenario-2-multi-conflict-same-file"])
        .expect("checkout feature branch should succeed");
    fs::write(
        &jokes_path,
        format!(
            "{}Why do Python developers make bad partners?,They only speak one language\n",
            base
        ),
    )
    .expect("write first feature joke");
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-programming.csv"])
        .expect("first feature AI checkpoint should succeed");
    repo.stage_all_and_commit("Add Python joke")
        .expect("first feature commit should succeed");

    fs::write(
        &jokes_path,
        format!(
            "{}Why do Python developers make bad partners?,They only speak one language\nHow many Rust developers does it take to change a lightbulb?,Two one to change it and one to write a song about the old one\n",
            base
        ),
    )
    .expect("write second feature joke");
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-programming.csv"])
        .expect("second feature AI checkpoint should succeed");
    repo.stage_all_and_commit("Add Rust joke")
        .expect("second feature commit should succeed");

    repo.git(&["checkout", &default_branch])
        .expect("checkout default branch should succeed");
    let main = format!(
        "{}Why do C++ developers get halloween mixed up with christmas?,Because Oct31 equals Dec25\nWhy did the developer go broke?,Because he used up all his cache\n",
        base
    );
    fs::write(&jokes_path, &main).expect("write main jokes");
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-programming.csv"])
        .expect("main AI checkpoint should succeed");
    repo.stage_all_and_commit("Add C++ jokes")
        .expect("main commit should succeed");

    let rebase_result = repo.git(&[
        "rebase",
        &default_branch,
        "scenario-2-multi-conflict-same-file",
    ]);
    assert!(
        rebase_result.is_err(),
        "first rebase stop should conflict on the Python joke"
    );

    fs::write(&jokes_path, &main).expect("resolve first conflict by keeping main side");
    repo.git(&["add", "jokes-programming.csv"])
        .expect("stage first conflict resolution");
    let second_stop = repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None);
    assert!(
        second_stop.is_err(),
        "continuing the empty first resolution should stop on the Rust conflict"
    );

    let rewritten = format!(
        "{}Why do C++ developers get halloween mixed up with christmas?,Because Oct31 equals Dec25\nWhy did the developer go broke?,Because he used up all his cache\nWhat's a programmer's favorite hangout place?,Foo Bar\n",
        base
    );
    checkpoint_claude_file_edit(
        &repo,
        "PreToolUse",
        "jokes-programming.csv",
        "resolve-second",
    );
    fs::write(&jokes_path, rewritten).expect("rewrite second conflict resolution");
    checkpoint_claude_file_edit(
        &repo,
        "PostToolUse",
        "jokes-programming.csv",
        "resolve-second",
    );
    repo.git(&["add", "jokes-programming.csv"])
        .expect("stage AI conflict resolution");
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should finish");

    let mut jokes = repo.filename("jokes-programming.csv");
    jokes.assert_committed_lines(crate::lines![
        "setup,punchline".ai(),
        "How many programmers does it take to change a light bulb?,None that's a hardware problem"
            .ai(),
        "Why do Java developers wear glasses?,Because they don't C#".ai(),
        "Why did the programmer quit his job?,Because he didn't get arrays".ai(),
        "Why do C++ developers get halloween mixed up with christmas?,Because Oct31 equals Dec25"
            .ai(),
        "Why did the developer go broke?,Because he used up all his cache".ai(),
        "What's a programmer's favorite hangout place?,Foo Bar".ai(),
    ]);
}

#[test]
fn test_regular_rebase_with_conflict_abort_preserves_original_notes() {
    let setup = setup_regular_rebase_conflict();
    let repo = setup.repo;

    // Verify AI commit has authorship notes before rebase
    let pre_rebase_note = repo.read_authorship_note(&setup.feature_ai_commit_sha);
    assert!(
        pre_rebase_note.is_some(),
        "Feature AI commit should have authorship notes before rebase"
    );

    // Rebase feature onto main — should conflict on shared.txt
    let rebase_result = repo.git(&["rebase", &setup.default_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should fail due to conflict on shared.txt"
    );

    // Abort the rebase
    repo.git(&["rebase", "--abort"])
        .expect("rebase --abort should succeed");

    // Verify HEAD is back to original feature SHA
    let current_head = repo
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    assert_eq!(
        current_head, setup.feature_ai_commit_sha,
        "HEAD should be back to original feature commit after abort"
    );

    // Verify authorship notes on the original SHA are still intact
    let post_abort_note = repo.read_authorship_note(&setup.feature_ai_commit_sha);
    assert!(
        post_abort_note.is_some(),
        "Feature AI commit should still have authorship notes after abort"
    );
}

crate::reuse_tests_in_worktree!(
    test_regular_rebase_with_conflict_preserves_ai_notes,
    test_regular_rebase_two_conflicts_ai_rewrite_after_skipped_conflict_is_attributed,
    test_regular_rebase_two_conflicts_ai_rewrite_after_empty_continue_is_attributed,
    test_regular_rebase_with_conflict_abort_preserves_original_notes,
);
