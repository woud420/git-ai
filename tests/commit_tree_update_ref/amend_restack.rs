use super::*;

#[test]
fn test_soft_reset_amend_then_branch_move_preserves_squashed_child_attribution() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    repo.git(&["checkout", "-b", "parent"])
        .expect("checkout parent should succeed");
    let mut parent_file = repo.filename("csf_parent.txt");
    parent_file.set_contents(lines!["parent line 1", "parent line 2"]);
    repo.stage_all_and_commit("parent")
        .expect("parent commit should succeed");

    repo.git(&["checkout", "-b", "child"])
        .expect("checkout child should succeed");
    let mut child_file = repo.filename("csf_child.txt");
    child_file.set_contents(lines!["child ai 1".ai()]);
    let child_one = repo
        .stage_all_and_commit("child commit 1")
        .expect("child commit 1 should succeed");

    child_file.set_contents(lines!["child ai 1".ai(), "child ai 2".ai()]);
    repo.stage_all_and_commit("child commit 2")
        .expect("child commit 2 should succeed");

    repo.sync_daemon();
    let reset_session = new_daemon_test_sync_session_id();
    let amend_session = new_daemon_test_sync_session_id();
    let switch_session = new_daemon_test_sync_session_id();

    raw_traced_git_with_session(
        &repo,
        &["reset", "--soft", &child_one.commit_sha],
        &reset_session,
    );
    raw_traced_git_with_session(
        &repo,
        &["commit", "--amend", "-m", "squashed child"],
        &amend_session,
    );
    raw_traced_git_with_session(&repo, &["switch", "-C", "parent", "HEAD"], &switch_session);
    repo.sync_daemon_external_completion_sessions(&[reset_session, amend_session, switch_session]);

    parent_file.assert_lines_and_blame(lines!["parent line 1".human(), "parent line 2".human(),]);
    child_file.assert_lines_and_blame(lines!["child ai 1".ai(), "child ai 2".ai()]);
}

#[test]
fn test_delayed_soft_reset_amend_then_branch_move_preserves_squashed_child_attribution() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    repo.git(&["checkout", "-b", "parent"])
        .expect("checkout parent should succeed");
    let mut parent_file = repo.filename("delayed_csf_parent.txt");
    parent_file.set_contents(lines!["parent line 1", "parent line 2"]);
    repo.stage_all_and_commit("parent")
        .expect("parent commit should succeed");

    repo.git(&["checkout", "-b", "child"])
        .expect("checkout child should succeed");
    let mut child_file = repo.filename("delayed_csf_child.txt");
    child_file.set_contents(lines!["child ai 1".ai()]);
    let child_one = repo
        .stage_all_and_commit("child commit 1")
        .expect("child commit 1 should succeed");

    child_file.set_contents(lines!["child ai 1".ai(), "child ai 2".ai()]);
    repo.stage_all_and_commit("child commit 2")
        .expect("child commit 2 should succeed");

    repo.sync_daemon();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let reset_trace = trace_dir.path().join("soft-reset.trace2");
    let amend_trace = trace_dir.path().join("amend.trace2");
    let switch_trace = trace_dir.path().join("switch.trace2");
    let reset_session = new_daemon_test_sync_session_id();
    let amend_session = new_daemon_test_sync_session_id();
    let switch_session = new_daemon_test_sync_session_id();
    let reset_session_arg = format!("git-ai.testSyncSession={reset_session}");
    let amend_session_arg = format!("git-ai.testSyncSession={amend_session}");
    let switch_session_arg = format!("git-ai.testSyncSession={switch_session}");

    raw_git_trace_to_file(
        &repo,
        &[
            "-c",
            &reset_session_arg,
            "reset",
            "--soft",
            &child_one.commit_sha,
        ],
        &reset_trace,
    );
    raw_git_trace_to_file(
        &repo,
        &[
            "-c",
            &amend_session_arg,
            "commit",
            "--amend",
            "-m",
            "squashed child",
        ],
        &amend_trace,
    );
    raw_git_trace_to_file(
        &repo,
        &["-c", &switch_session_arg, "switch", "-C", "parent", "HEAD"],
        &switch_trace,
    );

    replay_trace_file_to_daemon(&repo, &reset_trace);
    replay_trace_file_to_daemon(&repo, &amend_trace);
    replay_trace_file_to_daemon(&repo, &switch_trace);
    repo.sync_daemon_external_completion_sessions(&[reset_session, amend_session, switch_session]);

    parent_file.assert_lines_and_blame(lines!["parent line 1".human(), "parent line 2".human(),]);
    child_file.assert_lines_and_blame(lines!["child ai 1".ai(), "child ai 2".ai()]);
}

#[test]
fn test_reset_keep_rewrite_preserves_authorship_notes_on_current_branch() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    repo.git(&["checkout", "-b", "feature"])
        .expect("checkout feature should succeed");

    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(lines!["human line", "ai line".ai()]);
    let feature_commit = repo
        .stage_all_and_commit("feature commit")
        .expect("feature commit should succeed");

    let git_ai_repo = open_repo(&repo);
    assert!(
        read_note(&git_ai_repo, &feature_commit.commit_sha).is_some(),
        "expected initial feature commit to have an authorship note",
    );

    repo.git(&["checkout", "main"])
        .expect("checkout main should succeed");
    let mut trunk_file = repo.filename("trunk.txt");
    trunk_file.set_contents(lines!["trunk update"]);
    let main_commit = repo
        .stage_all_and_commit("main update")
        .expect("main update should succeed");

    repo.git(&["checkout", "feature"])
        .expect("checkout feature should succeed");
    let old_head = head_sha(&repo);
    let new_head =
        commit_tree_from_existing_tree(&repo, &old_head, &main_commit.commit_sha, "feature commit");

    repo.git(&["reset", "--keep", &new_head])
        .expect("git reset --keep should succeed");

    repo.sync_daemon();

    let git_ai_repo = open_repo(&repo);
    assert!(
        read_note(&git_ai_repo, &new_head).is_some(),
        "expected rewritten current-branch commit {} to preserve authorship note from {}",
        new_head,
        old_head,
    );

    let mut rewritten_file = repo.filename("feature.txt");
    rewritten_file.assert_lines_and_blame(lines!["human line".human(), "ai line".ai()]);
}

#[test]
fn test_update_ref_restack_after_parent_amend_preserves_child_attribution() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    repo.git(&["checkout", "-b", "parent"])
        .expect("checkout parent should succeed");
    let mut parent_file = repo.filename("parent.txt");
    parent_file.set_contents(lines!["parent ai".ai(), "parent human"]);
    let parent_commit = repo
        .stage_all_and_commit("parent")
        .expect("parent commit should succeed");

    repo.git(&["checkout", "-b", "child"])
        .expect("checkout child should succeed");
    let mut child_file = repo.filename("child.txt");
    child_file.set_contents(lines!["child ai".ai(), "child human"]);
    let child_commit = repo
        .stage_all_and_commit("child")
        .expect("child commit should succeed");

    let git_ai_repo = open_repo(&repo);
    assert!(
        read_note(&git_ai_repo, &child_commit.commit_sha).is_some(),
        "expected initial child commit to have an authorship note",
    );

    repo.git(&["checkout", "parent"])
        .expect("checkout parent should succeed");
    let mut parent_file2 = repo.filename("parent2.txt");
    parent_file2.set_contents(lines!["parent2 ai".ai()]);
    repo.git(&["add", "-A"]).expect("git add should succeed");
    repo.git(&["commit", "--amend", "-m", "modified parent"])
        .expect("git commit --amend should succeed");

    let amended_parent_head = head_sha(&repo);
    assert_ne!(
        amended_parent_head, parent_commit.commit_sha,
        "expected parent amend to rewrite the parent branch"
    );

    let new_child_head = plumbing_restack_child_branch(
        &repo,
        "child",
        &child_commit.commit_sha,
        &amended_parent_head,
        "child",
    );

    repo.sync_daemon();

    let git_ai_repo = open_repo(&repo);
    assert!(
        read_note(&git_ai_repo, &new_child_head).is_some(),
        "expected rewritten child commit {} to preserve authorship note from {}",
        new_child_head,
        child_commit.commit_sha,
    );

    repo.git(&["checkout", "child"])
        .expect("checkout child should succeed");
    let mut rewritten_child_file = repo.filename("child.txt");
    rewritten_child_file.assert_lines_and_blame(lines!["child ai".ai(), "child human".human()]);
}
