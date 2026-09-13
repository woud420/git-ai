use super::*;

#[test]
#[ignore = "stock trace2 does not record rebased output commit oids"]
fn test_delayed_rebase_trace_replay_preserves_rebased_commit_attribution() {
    let repo = TestRepo::new();
    let mut file = repo.filename("feature.txt");

    file.set_contents(lines!["base"]);
    repo.stage_all_and_commit("base").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.set_contents(lines!["base", "feature ai".ai()]);
    let original_feature = repo.stage_all_and_commit("feature ai").unwrap();

    repo.git(&["checkout", &default_branch]).unwrap();
    fs::write(repo.path().join("upstream.txt"), "upstream\n").unwrap();
    repo.stage_all_and_commit("upstream").unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let rebase_trace = trace_dir.path().join("rebase.trace2");

    raw_git_trace_to_file(&repo, &["rebase", &default_branch], &rebase_trace);
    let rebased_feature = head_sha(&repo);
    assert_ne!(original_feature.commit_sha, rebased_feature);

    fs::write(repo.path().join("later.txt"), "later\n").unwrap();
    repo.git_og(&["add", "later.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "later untraced commit"])
        .unwrap();

    replay_trace_file_to_daemon(&repo, &rebase_trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);

    assert_note_has_ai_for_file(&repo, &rebased_feature, "feature.txt");
}

#[test]
fn test_delayed_reset_trace_replay_reconstructs_reset_working_log_attribution() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    let mut file = repo.filename("reset-delayed.txt");
    file.set_contents(lines!["reset delayed ai".ai()]);
    let original_commit = repo.stage_all_and_commit("reset delayed ai").unwrap();

    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let reset_trace = trace_dir.path().join("reset.trace2");

    raw_git_trace_to_file(&repo, &["reset", "--mixed", "HEAD~1"], &reset_trace);
    replay_trace_file_to_daemon(&repo, &reset_trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);

    let recommit = repo.stage_all_and_commit("recommit reset work").unwrap();
    assert_ne!(original_commit.commit_sha, recommit.commit_sha);
    file.assert_committed_lines(lines!["reset delayed ai".ai()]);
}

#[test]
fn test_delayed_cherry_pick_trace_replay_preserves_picked_commit_attribution() {
    let repo = TestRepo::new();
    let mut file = repo.filename("picked.txt");

    file.set_contents(lines!["base"]);
    repo.stage_all_and_commit("base").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.set_contents(lines!["base", "picked ai".ai()]);
    let source = repo.stage_all_and_commit("picked ai").unwrap();

    repo.git(&["checkout", &default_branch]).unwrap();
    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let cherry_pick_trace = trace_dir.path().join("cherry-pick.trace2");

    raw_git_trace_to_file(
        &repo,
        &["cherry-pick", &source.commit_sha],
        &cherry_pick_trace,
    );
    let picked_commit = head_sha(&repo);

    fs::write(repo.path().join("later.txt"), "later\n").unwrap();
    repo.git_og(&["add", "later.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "later untraced commit"])
        .unwrap();

    replay_trace_file_to_daemon(&repo, &cherry_pick_trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);

    assert_note_has_ai_for_file(&repo, &picked_commit, "picked.txt");
}

#[test]
fn test_delayed_multi_cherry_pick_trace_replay_starts_at_first_pick_when_intermediate_ref_known() {
    let repo = TestRepo::new();
    let mut file = repo.filename("multi-picked.txt");

    file.set_contents(lines!["base"]);
    let base = repo.stage_all_and_commit("base").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.set_contents(lines!["base", "first picked ai".ai()]);
    repo.stage_all_and_commit("first picked ai").unwrap();
    let source_one = head_sha(&repo);
    file.set_contents(lines![
        "base",
        "first picked ai".ai(),
        "second picked ai".ai(),
    ]);
    repo.stage_all_and_commit("second picked ai").unwrap();
    let source_two = head_sha(&repo);

    repo.git(&["checkout", &default_branch]).unwrap();
    repo.sync_daemon();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let cherry_pick_trace = trace_dir.path().join("multi-cherry-pick.trace2");
    let session = new_daemon_test_sync_session_id();
    let session_arg = format!("git-ai.testSyncSession={session}");
    raw_git_trace_to_file(
        &repo,
        &["-c", &session_arg, "cherry-pick", &source_one, &source_two],
        &cherry_pick_trace,
    );
    let picked_commits = repo
        .git_og(&[
            "rev-list",
            "--reverse",
            &format!("{}..HEAD", base.commit_sha),
        ])
        .expect("rev-list picked commits should succeed")
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(picked_commits.len(), 2);

    repo.git(&["branch", "known-intermediate-pick", &picked_commits[0]])
        .expect("creating intermediate branch should succeed");
    repo.sync_daemon();

    replay_trace_file_to_daemon(&repo, &cherry_pick_trace);
    repo.sync_daemon_external_completion_sessions(&[session]);

    assert_note_has_ai_for_file(&repo, &picked_commits[0], "multi-picked.txt");
    assert_note_has_ai_for_file(&repo, &picked_commits[1], "multi-picked.txt");
    file.assert_committed_lines(lines![
        "base".ai(),
        "first picked ai".ai(),
        "second picked ai".ai(),
    ]);
}

#[test]
fn test_delayed_failed_cherry_pick_with_unresolved_source_does_not_consume_later_pick() {
    let repo = TestRepo::new();
    let mut file = repo.filename("file.txt");

    file.set_contents(lines!["base line"]);
    repo.stage_all_and_commit("initial").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, lines!["AI line 1".ai()]);
    repo.stage_all_and_commit("AI commit 1").unwrap();
    let source_one = head_sha(&repo);

    file.insert_at(2, lines!["AI line 2".ai()]);
    repo.stage_all_and_commit("AI commit 2").unwrap();
    let source_two = head_sha(&repo);

    repo.git(&["checkout", &default_branch]).unwrap();
    repo.sync_daemon();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let failed_trace = trace_dir.path().join("failed-cherry-pick.trace2");
    let good_trace = trace_dir.path().join("good-cherry-pick.trace2");
    let failed_session = new_daemon_test_sync_session_id();
    let good_session = new_daemon_test_sync_session_id();
    let failed_session_arg = format!("git-ai.testSyncSession={failed_session}");
    let good_session_arg = format!("git-ai.testSyncSession={good_session}");
    let bad_source_arg = format!("{source_one} {source_two}");

    let failed = raw_git_trace_to_file_output(
        &repo,
        &["-c", &failed_session_arg, "cherry-pick", &bad_source_arg],
        &failed_trace,
    );
    assert!(
        !failed.status.success(),
        "combined cherry-pick source should be invalid\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );

    raw_git_trace_to_file(
        &repo,
        &["-c", &good_session_arg, "cherry-pick", &source_one],
        &good_trace,
    );
    let picked_commit = head_sha(&repo);

    replay_trace_file_to_daemon(&repo, &failed_trace);
    replay_trace_file_to_daemon(&repo, &good_trace);
    repo.sync_daemon_external_completion_sessions(&[failed_session, good_session]);

    assert_note_has_ai_for_file(&repo, &picked_commit, "file.txt");
    file.assert_committed_lines(lines!["base line".ai(), "AI line 1".ai()]);
}

#[test]
fn test_delayed_pull_rebase_trace_replay_starts_at_start_when_intermediate_ref_known() {
    let (local, _upstream) = TestRepo::new_with_remote();
    let mut file = local.filename("pull-rebase-picked.txt");

    file.set_contents(lines!["base"]);
    let initial = local.stage_all_and_commit("initial").unwrap();
    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push initial commit should succeed");

    file.set_contents(lines!["base", "first local ai".ai()]);
    local.stage_all_and_commit("first local ai").unwrap();
    file.set_contents(lines![
        "base",
        "first local ai".ai(),
        "second local ai".ai(),
    ]);
    let local_tip = local.stage_all_and_commit("second local ai").unwrap();
    let branch = local.current_branch();

    local
        .git(&["reset", "--hard", &initial.commit_sha])
        .expect("reset to initial should succeed");
    let mut upstream_file = local.filename("pull-rebase-upstream.txt");
    upstream_file.set_contents(lines!["upstream"]);
    let upstream_tip = local.stage_all_and_commit("upstream").unwrap();
    local
        .git(&["push", "--force", "origin", &format!("HEAD:{}", branch)])
        .expect("push upstream divergence should succeed");
    local
        .git(&["reset", "--hard", &local_tip.commit_sha])
        .expect("reset to local tip should succeed");
    local.sync_daemon();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let pull_trace = trace_dir.path().join("pull-rebase.trace2");
    let session = new_daemon_test_sync_session_id();
    let session_arg = format!("git-ai.testSyncSession={session}");
    raw_git_trace_to_file(
        &local,
        &["-c", &session_arg, "pull", "--rebase", "origin", &branch],
        &pull_trace,
    );
    let rebased_commits = local
        .git_og(&[
            "rev-list",
            "--reverse",
            &format!("{}..HEAD", upstream_tip.commit_sha),
        ])
        .expect("rev-list rebased commits should succeed")
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(rebased_commits.len(), 2);

    local
        .git(&[
            "branch",
            "known-pull-rebase-intermediate",
            &rebased_commits[0],
        ])
        .expect("creating intermediate pull branch should succeed");
    local.sync_daemon();

    replay_trace_file_to_daemon(&local, &pull_trace);
    local.sync_daemon_external_completion_sessions(&[session]);

    assert_note_has_ai_for_file(&local, &rebased_commits[0], "pull-rebase-picked.txt");
    assert_note_has_ai_for_file(&local, &rebased_commits[1], "pull-rebase-picked.txt");
    file.assert_committed_lines(lines![
        "base".ai(),
        "first local ai".ai(),
        "second local ai".ai(),
    ]);
}

#[test]
fn test_delayed_multi_revert_trace_replay_starts_at_first_revert_when_intermediate_ref_known() {
    let repo = TestRepo::new();
    let mut first_file = repo.filename("multi-reverted-first.txt");
    let mut second_file = repo.filename("multi-reverted-second.txt");

    first_file.set_contents(lines!["first revert-restored ai".ai()]);
    let first_ai = repo.stage_all_and_commit("first ai").unwrap();
    first_file.set_contents(lines!["first human replacement"]);
    let replace_first = repo.stage_all_and_commit("replace first ai").unwrap();
    second_file.set_contents(lines!["second revert-restored ai".ai()]);
    let second_ai = repo.stage_all_and_commit("second ai").unwrap();
    second_file.set_contents(lines!["second human replacement"]);
    let replace_second = repo.stage_all_and_commit("replace second ai").unwrap();
    assert_note_has_ai_for_file(&repo, &first_ai.commit_sha, "multi-reverted-first.txt");
    assert_note_has_ai_for_file(&repo, &second_ai.commit_sha, "multi-reverted-second.txt");
    repo.sync_daemon();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let revert_trace = trace_dir.path().join("multi-revert.trace2");
    let session = new_daemon_test_sync_session_id();
    let session_arg = format!("git-ai.testSyncSession={session}");
    raw_git_trace_to_file(
        &repo,
        &[
            "-c",
            &session_arg,
            "revert",
            "--no-edit",
            &replace_second.commit_sha,
            &replace_first.commit_sha,
        ],
        &revert_trace,
    );
    let revert_commits = repo
        .git_og(&[
            "rev-list",
            "--reverse",
            &format!("{}..HEAD", replace_second.commit_sha),
        ])
        .expect("rev-list revert commits should succeed")
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(revert_commits.len(), 2);

    repo.git(&["branch", "known-intermediate-revert", &revert_commits[0]])
        .expect("creating intermediate revert branch should succeed");
    repo.sync_daemon();

    replay_trace_file_to_daemon(&repo, &revert_trace);
    repo.sync_daemon_external_completion_sessions(&[session]);

    assert_note_has_ai_for_file(&repo, &revert_commits[0], "multi-reverted-second.txt");
    assert_note_has_ai_for_file(&repo, &revert_commits[1], "multi-reverted-first.txt");
    first_file.assert_committed_lines(lines!["first revert-restored ai".ai()]);
    second_file.assert_committed_lines(lines!["second revert-restored ai".ai()]);
}
