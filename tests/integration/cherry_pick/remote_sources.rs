use super::{ExpectedLineExt, TestRepo, fs, git_common_dir};

/// Regression test for #955: cherry-pick from a remote repo whose notes have not been
/// fetched locally should still produce correct AI attribution.
///
/// Bug: the post-cherry-pick hook looked up notes for the source commit to copy
/// attribution, but found nothing because `refs/notes/ai` hadn't been fetched from the
/// remote.  The fix auto-fetches notes via `fetch_authorship_notes` (the safe,
/// non-destructive pattern) when any source commit is missing local notes.
#[test]
fn test_cherry_pick_from_remote_without_prefetched_notes() {
    // Source repo: one human initial commit, then one AI commit.
    let source_repo = TestRepo::new();
    let mut source_file = source_repo.filename("file.txt");
    source_file.set_contents(crate::lines!["base"]);
    source_repo.stage_all_and_commit("initial").unwrap();
    source_file.insert_at(1, crate::lines!["AI line".ai()]);
    source_repo.stage_all_and_commit("AI commit").unwrap();
    let ai_commit = source_repo
        .git(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Target repo: independent repo with the same base content so the cherry-pick
    // diff ("add AI line after base") applies cleanly.
    let target_repo = TestRepo::new();
    let mut target_file = target_repo.filename("file.txt");
    target_file.set_contents(crate::lines!["base"]);
    target_repo.stage_all_and_commit("initial").unwrap();

    // Register source as a remote and fetch commits — but NOT refs/notes/ai.
    // git fetch only fetches what the refspec asks for; notes are separate.
    target_repo
        .git(&[
            "remote",
            "add",
            "source",
            source_repo.path().to_str().unwrap(),
        ])
        .unwrap();
    // Fetch only the branch objects, explicitly excluding notes.
    target_repo
        .git(&["fetch", "source", "refs/heads/*:refs/remotes/source/*"])
        .unwrap();

    // Confirm notes are absent (the fix relies on detecting this absence).
    let _ = target_repo.git(&["update-ref", "-d", "refs/notes/ai"]);
    let _ = target_repo.git(&["update-ref", "-d", "refs/notes/ai-remote/source"]);

    // Cherry-pick the AI commit.  The fix should auto-fetch notes from "source"
    // and produce correct attribution.
    target_repo.git(&["cherry-pick", &ai_commit]).unwrap();

    // Single-commit cherry-pick: the source note covers all file content, so both lines
    // end up AI-attributed after the note is copied to the cherry-picked commit.
    target_file.assert_lines_and_blame(crate::lines!["base".ai(), "AI line".ai(),]);
}

#[test]
fn test_cherry_pick_local_remote_tracking_ref_missing_from_daemon_snapshot() {
    let repo = TestRepo::new();
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("initial").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI line".ai()]);
    let source_commit = repo.stage_all_and_commit("AI commit").unwrap();

    repo.read_authorship_note(&source_commit.commit_sha)
        .expect("source authorship note should already be local");

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git_og(&[
        "update-ref",
        "refs/remotes/origin/feature",
        &source_commit.commit_sha,
    ])
    .unwrap();

    repo.git(&["cherry-pick", "origin/feature"]).unwrap();

    file.assert_lines_and_blame(crate::lines!["base".ai(), "AI line".ai(),]);
}

#[test]
fn test_cherry_pick_partial_remote_tracking_ref_resolution_falls_back_to_git() {
    let repo = TestRepo::new();
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("initial").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, crate::lines!["AI line 1".ai()]);
    let first_source_commit = repo.stage_all_and_commit("AI commit 1").unwrap();
    file.insert_at(2, crate::lines!["AI line 2".ai()]);
    let second_source_commit = repo.stage_all_and_commit("AI commit 2").unwrap();

    repo.read_authorship_note(&first_source_commit.commit_sha)
        .expect("first source authorship note should already be local");
    repo.read_authorship_note(&second_source_commit.commit_sha)
        .expect("second source authorship note should already be local");

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git_og(&[
        "update-ref",
        "refs/remotes/origin/feature",
        &second_source_commit.commit_sha,
    ])
    .unwrap();

    repo.git(&[
        "cherry-pick",
        &first_source_commit.commit_sha,
        "origin/feature",
    ])
    .unwrap();

    file.assert_lines_and_blame(crate::lines![
        "base".ai(),
        "AI line 1".ai(),
        "AI line 2".ai(),
    ]);
}

#[test]
fn test_cherry_pick_failed_continue_keeps_pending_remote_tracking_source() {
    let repo = TestRepo::new();
    let mut file = repo.filename("file.txt");
    file.set_contents(crate::lines!["Line 1", "Line 2", "Line 3"]);
    repo.stage_all_and_commit("initial").unwrap();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.replace_at(1, "AI_REMOTE_VERSION".ai());
    let source_commit = repo.stage_all_and_commit("AI feature").unwrap();
    repo.read_authorship_note(&source_commit.commit_sha)
        .expect("source authorship note should already be local");

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git_og(&[
        "update-ref",
        "refs/remotes/origin/feature",
        &source_commit.commit_sha,
    ])
    .unwrap();

    file.replace_at(1, "MAIN_BRANCH_VERSION".human());
    repo.stage_all_and_commit("Human change").unwrap();

    let cherry_pick_result = repo.git(&["cherry-pick", "origin/feature"]);
    assert!(cherry_pick_result.is_err(), "cherry-pick should conflict");
    repo.sync_daemon();

    let failed_continue = repo.git(&["cherry-pick", "--continue"]);
    assert!(
        failed_continue.is_err(),
        "unresolved cherry-pick should not continue"
    );
    repo.sync_daemon();

    fs::write(
        repo.path().join("file.txt"),
        "Line 1\nAI_REMOTE_VERSION\nLine 3",
    )
    .unwrap();
    repo.git(&["add", "file.txt"]).unwrap();
    repo.git(&["cherry-pick", "--continue"]).unwrap();

    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "AI_REMOTE_VERSION".ai(),
        "Line 3".human(),
    ]);
}

#[test]
fn test_cherry_pick_skip_failed_next_conflict_advances_pending_remote_tracking_source() {
    let repo = TestRepo::new();
    let conflict_a_path = repo.path().join("conflict_a.txt");
    let conflict_b_path = repo.path().join("conflict_b.txt");

    fs::write(&conflict_a_path, "base\nshared\n").unwrap();
    fs::write(&conflict_b_path, "base\nshared\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "conflict_a.txt"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "conflict_b.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial").unwrap();
    let mut conflict_a = repo.filename("conflict_a.txt");
    let mut conflict_b = repo.filename("conflict_b.txt");
    conflict_a.assert_committed_lines(crate::lines!["base".human(), "shared".human(),]);
    conflict_b.assert_committed_lines(crate::lines!["base".human(), "shared".human(),]);
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    repo.human_edit("conflict_a.txt", "base\nFEATURE_A_HUMAN\n");
    let skipped_source_commit = repo.stage_all_and_commit("human conflict A").unwrap();
    conflict_a.assert_committed_lines(crate::lines!["base".human(), "FEATURE_A_HUMAN".human(),]);

    fs::write(&conflict_b_path, "base\nAI_REMOTE_VERSION\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict_b.txt"])
        .unwrap();
    let applied_source_commit = repo.stage_all_and_commit("AI conflict B").unwrap();
    conflict_b.assert_committed_lines(crate::lines!["base".human(), "AI_REMOTE_VERSION".ai(),]);

    repo.read_authorship_note(&skipped_source_commit.commit_sha)
        .expect("skipped source authorship note should already be local");
    repo.read_authorship_note(&applied_source_commit.commit_sha)
        .expect("applied source authorship note should already be local");

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.git_og(&[
        "update-ref",
        "refs/remotes/origin/feature",
        &applied_source_commit.commit_sha,
    ])
    .unwrap();

    repo.human_edit("conflict_a.txt", "base\nMAIN_A_HUMAN\n");
    repo.human_edit("conflict_b.txt", "base\nMAIN_B_HUMAN\n");
    repo.stage_all_and_commit("main conflicts").unwrap();
    conflict_a.assert_committed_lines(crate::lines!["base".human(), "MAIN_A_HUMAN".human(),]);
    conflict_b.assert_committed_lines(crate::lines!["base".human(), "MAIN_B_HUMAN".human(),]);

    let cherry_pick_result = repo.git(&["cherry-pick", "origin/feature~1", "origin/feature"]);
    assert!(
        cherry_pick_result.is_err(),
        "first cherry-pick should conflict"
    );
    repo.sync_daemon();

    let skip_result = repo.git(&["cherry-pick", "--skip"]);
    assert!(
        skip_result.is_err(),
        "skip should advance to the second source and conflict"
    );
    repo.sync_daemon();

    fs::write(&conflict_b_path, "base\nAI_REMOTE_VERSION\n").unwrap();
    repo.git(&["add", "conflict_b.txt"]).unwrap();
    repo.git(&["cherry-pick", "--continue"]).unwrap();

    conflict_a.assert_committed_lines(crate::lines!["base".human(), "MAIN_A_HUMAN".human(),]);
    conflict_b.assert_committed_lines(crate::lines!["base".human(), "AI_REMOTE_VERSION".ai(),]);
}

#[test]
fn test_cherry_pick_skip_failed_next_conflict_does_not_double_skip_refcursor_sources() {
    let repo = TestRepo::new();
    let conflict_a_path = repo.path().join("conflict_a.txt");
    let clean_b_path = repo.path().join("clean_b.txt");
    let conflict_c_path = repo.path().join("conflict_c.txt");

    fs::write(&conflict_a_path, "base\nshared\n").unwrap();
    fs::write(&clean_b_path, "base\nshared\n").unwrap();
    fs::write(&conflict_c_path, "base\nshared\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "conflict_a.txt"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "clean_b.txt"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "conflict_c.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial").unwrap();
    let mut conflict_a = repo.filename("conflict_a.txt");
    let mut clean_b = repo.filename("clean_b.txt");
    let mut conflict_c = repo.filename("conflict_c.txt");
    conflict_a.assert_committed_lines(crate::lines!["base".human(), "shared".human(),]);
    clean_b.assert_committed_lines(crate::lines!["base".human(), "shared".human(),]);
    conflict_c.assert_committed_lines(crate::lines!["base".human(), "shared".human(),]);
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    repo.human_edit("conflict_a.txt", "base\nFEATURE_A_HUMAN\n");
    let skipped_source_commit = repo.stage_all_and_commit("human conflict A").unwrap();
    conflict_a.assert_committed_lines(crate::lines!["base".human(), "FEATURE_A_HUMAN".human(),]);

    fs::write(&clean_b_path, "base\nAI_B_VERSION\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "clean_b.txt"])
        .unwrap();
    let applied_during_skip_commit = repo.stage_all_and_commit("AI clean B").unwrap();
    clean_b.assert_committed_lines(crate::lines!["base".human(), "AI_B_VERSION".ai(),]);

    fs::write(&conflict_c_path, "base\nAI_C_VERSION\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "conflict_c.txt"])
        .unwrap();
    let next_conflict_commit = repo.stage_all_and_commit("AI conflict C").unwrap();
    conflict_c.assert_committed_lines(crate::lines!["base".human(), "AI_C_VERSION".ai(),]);

    repo.read_authorship_note(&skipped_source_commit.commit_sha)
        .expect("skipped source authorship note should already be local");
    repo.read_authorship_note(&applied_during_skip_commit.commit_sha)
        .expect("applied source authorship note should already be local");
    repo.read_authorship_note(&next_conflict_commit.commit_sha)
        .expect("next conflict source authorship note should already be local");

    repo.git(&["checkout", &main_branch]).unwrap();
    repo.human_edit("conflict_a.txt", "base\nMAIN_A_HUMAN\n");
    repo.human_edit("conflict_c.txt", "base\nMAIN_C_HUMAN\n");
    repo.stage_all_and_commit("main conflicts").unwrap();
    conflict_a.assert_committed_lines(crate::lines!["base".human(), "MAIN_A_HUMAN".human(),]);
    clean_b.assert_committed_lines(crate::lines!["base".human(), "shared".human(),]);
    conflict_c.assert_committed_lines(crate::lines!["base".human(), "MAIN_C_HUMAN".human(),]);

    let cherry_pick_result = repo.git(&[
        "cherry-pick",
        &skipped_source_commit.commit_sha,
        &applied_during_skip_commit.commit_sha,
        &next_conflict_commit.commit_sha,
    ]);
    assert!(
        cherry_pick_result.is_err(),
        "first cherry-pick should conflict"
    );
    repo.sync_daemon();

    let skip_result = repo.git(&["cherry-pick", "--skip"]);
    assert!(
        skip_result.is_err(),
        "skip should apply B and then conflict on C"
    );
    repo.sync_daemon();
    clean_b.assert_committed_lines(crate::lines!["base".human(), "AI_B_VERSION".ai(),]);

    fs::write(&conflict_c_path, "base\nAI_C_VERSION\n").unwrap();
    repo.git(&["add", "conflict_c.txt"]).unwrap();
    repo.git(&["cherry-pick", "--continue"]).unwrap();

    clean_b.assert_committed_lines(crate::lines!["base".human(), "AI_B_VERSION".ai(),]);
    conflict_c.assert_committed_lines(crate::lines!["base".human(), "AI_C_VERSION".ai(),]);
}

#[test]
fn test_cherry_pick_source_note_import_failure_is_limited_to_missing_note() {
    let source_repo = TestRepo::new();
    let mut source_file = source_repo.filename("file.txt");
    source_file.set_contents(crate::lines!["base"]);
    source_repo.stage_all_and_commit("initial").unwrap();
    source_file.insert_at(1, crate::lines!["AI line".ai()]);
    source_repo.stage_all_and_commit("AI commit").unwrap();
    let ai_commit = source_repo
        .git(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let target_repo = TestRepo::new();
    let mut target_file = target_repo.filename("file.txt");
    target_file.set_contents(crate::lines!["base"]);
    target_repo.stage_all_and_commit("initial").unwrap();

    target_repo
        .git(&[
            "remote",
            "add",
            "source",
            source_repo.path().to_str().unwrap(),
        ])
        .unwrap();
    target_repo
        .git(&["fetch", "source", "refs/heads/*:refs/remotes/source/*"])
        .unwrap();
    let _ = target_repo.git(&["update-ref", "-d", "refs/notes/ai"]);
    let _ = target_repo.git(&["update-ref", "-d", "refs/notes/ai-remote/source"]);

    let notes_dir = git_common_dir(&target_repo).join("refs/notes");
    fs::create_dir_all(&notes_dir).expect("notes dir should be creatable");
    fs::write(notes_dir.join("ai.lock"), "stale lock\n").expect("notes lock should be writable");

    target_repo.git(&["cherry-pick", &ai_commit]).unwrap();
    target_repo.sync_daemon_force();

    target_file.assert_committed_lines(crate::lines![
        "base".human(),
        "AI line".unattributed_human(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_cherry_pick_from_remote_without_prefetched_notes,
    test_cherry_pick_skip_failed_next_conflict_advances_pending_remote_tracking_source,
    test_cherry_pick_skip_failed_next_conflict_does_not_double_skip_refcursor_sources,
    test_cherry_pick_source_note_import_failure_is_limited_to_missing_note,
);
