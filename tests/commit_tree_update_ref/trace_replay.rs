use super::{
    ExpectedLineExt, TRACE_ROOT_REFLOG_START_OFFSETS_FIELD, TestRepo, Value,
    assert_note_has_ai_for_file, current_reflog_offsets, fs, head_sha, json,
    new_daemon_test_sync_session_id, raw_git_trace_to_file, raw_traced_git, raw_untraced_git,
    replay_trace_file_to_daemon, replay_trace_payloads_to_daemon, setup_initial_commit,
};

#[test]
fn test_split_trace_metadata_still_sequences_amend_authorship() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    let mut file = repo.filename("split_trace.txt");
    file.set_contents(lines!["split trace ai".ai()]);
    repo.stage_all_and_commit("split trace base")
        .expect("base commit should succeed");
    file.assert_committed_lines(lines!["split trace ai".ai()]);
    repo.sync_daemon();

    let offsets = current_reflog_offsets(&repo);
    let session = new_daemon_test_sync_session_id();
    let session_arg = format!("git-ai.testSyncSession={session}");
    raw_untraced_git(&repo, &["commit", "--amend", "-m", "split trace amended"]);
    let amended = head_sha(&repo);

    let sid = "20260411T120000.000000-Psplitmetadata";
    let mut start = json!({
        "event": "start",
        "sid": sid,
        "argv": ["git", "-c", session_arg, "commit", "--amend", "-m", "split trace amended"],
        "time_ns": 2u64,
    });
    start.as_object_mut().unwrap().insert(
        TRACE_ROOT_REFLOG_START_OFFSETS_FIELD.to_string(),
        Value::Object(offsets),
    );
    replay_trace_payloads_to_daemon(
        &repo,
        &[
            json!({
                "event": "def_repo",
                "sid": sid,
                "worktree": repo.path().to_string_lossy().to_string(),
                "time_ns": 1u64,
            }),
            start,
            json!({
                "event": "atexit",
                "sid": sid,
                "code": 0,
                "time_ns": 3u64,
            }),
        ],
    );
    repo.sync_daemon_external_completion_sessions(&[session]);

    assert_note_has_ai_for_file(&repo, &amended, "split_trace.txt");
    file.assert_committed_lines(lines!["split trace ai".ai()]);
}

#[test]
fn test_back_to_back_raw_commits_do_not_span_later_ref_move() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    fs::write(repo.path().join("first.txt"), "first ai\n").unwrap();
    fs::write(repo.path().join("second.txt"), "second ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "first.txt"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "second.txt"])
        .unwrap();
    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    raw_untraced_git(&repo, &["add", "first.txt"]);
    raw_traced_git(&repo, &["commit", "-m", "first raw commit"]);
    let first_commit = head_sha(&repo);

    raw_untraced_git(&repo, &["add", "second.txt"]);
    raw_traced_git(&repo, &["commit", "-m", "second raw commit"]);
    let second_commit = head_sha(&repo);

    repo.wait_for_daemon_total_completion_count(baseline, baseline + 2);

    assert_note_has_ai_for_file(&repo, &first_commit, "first.txt");
    assert_note_has_ai_for_file(&repo, &second_commit, "second.txt");
}

#[test]
fn test_raw_commit_trace2_does_not_record_created_commit_oid() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    fs::write(repo.path().join("trace-only.txt"), "trace only\n").unwrap();
    raw_untraced_git(&repo, &["add", "trace-only.txt"]);

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let commit_trace = trace_dir.path().join("commit.trace2");

    raw_git_trace_to_file(&repo, &["commit", "-m", "trace only"], &commit_trace);
    let commit_sha = head_sha(&repo);
    let trace = fs::read_to_string(&commit_trace).expect("read trace2 file");

    assert!(
        !trace.contains(&commit_sha),
        "stock trace2 should not contain the created commit oid"
    );
}

#[test]
fn test_delayed_commit_trace_replay_attributes_matching_commit_not_later_commit() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    fs::write(repo.path().join("first-delayed.txt"), "first delayed ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "first-delayed.txt"])
        .unwrap();
    raw_untraced_git(&repo, &["add", "first-delayed.txt"]);
    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let commit_trace = trace_dir.path().join("commit.trace2");

    raw_git_trace_to_file(&repo, &["commit", "-m", "first delayed"], &commit_trace);
    let first_commit = head_sha(&repo);

    fs::write(repo.path().join("later-delayed.txt"), "later untraced\n").unwrap();
    raw_untraced_git(&repo, &["add", "later-delayed.txt"]);
    raw_untraced_git(&repo, &["commit", "-m", "later untraced commit"]);
    let later_commit = head_sha(&repo);

    replay_trace_file_to_daemon(&repo, &commit_trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);

    assert_note_has_ai_for_file(&repo, &first_commit, "first-delayed.txt");
    assert!(
        repo.read_authorship_note(&later_commit).is_none(),
        "delayed commit trace replay must not attach attribution to a later commit"
    );
}

#[cfg(not(windows))]
#[test]
fn test_trace_listener_bootstrap_captures_commit_ref_transition_before_worker_spawn_delay() {
    let repo = TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_TRACE_LISTENER_WORKER_SPAWN_DELAY_MS",
        "200",
    )]);
    fs::write(repo.path().join("README.md"), "base\n").unwrap();
    repo.git_og(&["add", "README.md"]).unwrap();
    repo.git_og(&["commit", "-m", "base"]).unwrap();

    fs::write(
        repo.path().join("bootstrap-race.txt"),
        "bootstrap race ai\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "bootstrap-race.txt"])
        .unwrap();
    repo.git(&["add", "bootstrap-race.txt"]).unwrap();
    let committed = repo.commit("bootstrap race").unwrap();

    assert_note_has_ai_for_file(&repo, &committed.commit_sha, "bootstrap-race.txt");
}

#[test]
#[ignore = "stock trace2 does not record merge --squash source oid after SQUASH_MSG is gone"]
fn test_delayed_squash_merge_trace_replay_preserves_source_attribution() {
    let repo = TestRepo::new();
    let mut file = repo.filename("main.txt");

    file.set_contents(lines!["base"]);
    repo.stage_all_and_commit("base").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(1, lines!["feature ai".ai()]);
    repo.stage_all_and_commit("feature ai").unwrap();

    repo.git(&["checkout", &default_branch]).unwrap();
    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let merge_trace = trace_dir.path().join("merge.trace2");
    let commit_trace = trace_dir.path().join("commit.trace2");

    raw_git_trace_to_file(&repo, &["merge", "--squash", "feature"], &merge_trace);
    raw_git_trace_to_file(&repo, &["commit", "-m", "squash feature"], &commit_trace);
    let squash_commit = head_sha(&repo);

    replay_trace_file_to_daemon(&repo, &merge_trace);
    replay_trace_file_to_daemon(&repo, &commit_trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 2);

    assert_note_has_ai_for_file(&repo, &squash_commit, "main.txt");
}
