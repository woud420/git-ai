use super::*;

#[test]
fn test_delayed_commit_trace_uses_committed_tree_not_later_worktree() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);
    let file_rel = "delayed-commit-race.txt";
    let file_path = repo.path().join(file_rel);

    fs::write(&file_path, "first ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", file_rel]).unwrap();
    repo.git_og(&["add", file_rel]).unwrap();
    repo.sync_daemon();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let commit_trace = trace_dir.path().join("commit.trace2");
    raw_git_trace_to_file(&repo, &["commit", "-m", "first ai"], &commit_trace);
    let first_commit = head_sha(&repo);

    fs::write(&file_path, "first ai\nsecond ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", file_rel]).unwrap();
    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    replay_trace_file_to_daemon(&repo, &commit_trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);
    repo.sync_daemon();

    let mut file = repo.filename(file_rel);
    file.assert_committed_lines(lines!["first ai".ai()]);

    repo.stage_all_and_commit("second ai")
        .expect("second commit should succeed");
    file.assert_committed_lines(lines!["first ai".ai(), "second ai".ai()]);

    assert_note_has_ai_for_file(&repo, &first_commit, file_rel);
}

#[test]
fn test_update_ref_head_with_new_content_then_amend_preserves_attribution() {
    use std::fs;

    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    let file_path = repo.path().join("feature.txt");

    // Write AI content and checkpoint
    fs::write(&file_path, "ai line 1\nai line 2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "feature.txt"])
        .unwrap();

    // Stage
    repo.git(&["add", "-A"]).unwrap();

    // Plumbing: write-tree, commit-tree, update-ref HEAD
    let parent_sha = head_sha(&repo);
    let tree_sha = repo.git(&["write-tree"]).unwrap().trim().to_string();
    let commit_sha = repo
        .git(&[
            "commit-tree",
            &tree_sha,
            "-p",
            &parent_sha,
            "-m",
            "plumbing commit",
        ])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["update-ref", "HEAD", &commit_sha, &parent_sha])
        .unwrap();

    let mut feature_file = repo.filename("feature.txt");
    feature_file.assert_lines_and_blame(lines!["ai line 1".ai(), "ai line 2".ai()]);
}

#[test]
fn test_update_ref_current_branch_with_new_content_preserves_attribution() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    repo.git(&["checkout", "-b", "feature"])
        .expect("checkout feature should succeed");

    fs::write(repo.path().join("branch-plumbing.txt"), "branch ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "branch-plumbing.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();

    let parent_sha = head_sha(&repo);
    let tree_sha = repo.git(&["write-tree"]).unwrap().trim().to_string();
    let commit_sha = repo
        .git(&[
            "commit-tree",
            &tree_sha,
            "-p",
            &parent_sha,
            "-m",
            "branch plumbing commit",
        ])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["update-ref", "refs/heads/feature", &commit_sha, &parent_sha])
        .unwrap();

    let mut feature_file = repo.filename("branch-plumbing.txt");
    feature_file.assert_lines_and_blame(lines!["branch ai".ai()]);
}

#[test]
fn test_update_ref_fast_forward_bounds_committed_hunks_to_final_commit() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    let file_rel = "ff-overlap.txt";
    let file_path = repo.path().join(file_rel);
    fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", file_rel])
        .unwrap();
    repo.stage_all_and_commit("add fast-forward overlap base")
        .unwrap();
    let mut file = repo.filename(file_rel);
    file.assert_committed_lines(lines!["base".human()]);

    let old_tip = head_sha(&repo);
    let final_content = "base\nintermediate pulled line\nfinal checkpointed line\n";
    fs::write(&file_path, final_content).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", file_rel]).unwrap();
    repo.sync_daemon();

    fs::write(&file_path, "base\nintermediate pulled line\n").unwrap();
    raw_untraced_git(&repo, &["add", file_rel]);
    let intermediate_tree = raw_untraced_git(&repo, &["write-tree"]).trim().to_string();
    let intermediate_commit = raw_untraced_git(
        &repo,
        &[
            "commit-tree",
            &intermediate_tree,
            "-p",
            &old_tip,
            "-m",
            "intermediate pulled commit",
        ],
    )
    .trim()
    .to_string();

    fs::write(&file_path, final_content).unwrap();
    raw_untraced_git(&repo, &["add", file_rel]);
    let final_tree = raw_untraced_git(&repo, &["write-tree"]).trim().to_string();
    let final_commit = raw_untraced_git(
        &repo,
        &[
            "commit-tree",
            &final_tree,
            "-p",
            &intermediate_commit,
            "-m",
            "final pulled commit",
        ],
    )
    .trim()
    .to_string();

    repo.git(&["update-ref", "HEAD", &final_commit, &old_tip])
        .unwrap();
    let note = repo
        .read_authorship_note(&final_commit)
        .expect("fast-forward final commit should have an authorship note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse authorship note");
    let ai_lines = ai_attested_lines_for_file(&log, file_rel);

    assert!(
        !ai_lines.contains(&2),
        "intermediate pulled line must not be attributed from the old-tip..new-head diff: {ai_lines:?}"
    );
    assert!(
        ai_lines.contains(&3),
        "final commit line should remain attributed to the checkpointed AI edit: {ai_lines:?}"
    );

    file.assert_committed_lines(lines![
        "base".human(),
        "intermediate pulled line".human(),
        "final checkpointed line".ai(),
    ]);
}

#[test]
fn test_delayed_current_branch_update_ref_trace_preserves_new_commit_attribution() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    repo.git(&["checkout", "-b", "feature"])
        .expect("checkout feature should succeed");

    fs::write(
        repo.path().join("delayed-branch-plumbing.txt"),
        "branch ai\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "delayed-branch-plumbing.txt"])
        .unwrap();
    raw_untraced_git(&repo, &["add", "-A"]);
    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    let parent_sha = head_sha(&repo);
    let tree_sha = raw_untraced_git(&repo, &["write-tree"]).trim().to_string();
    let commit_sha = raw_untraced_git(
        &repo,
        &[
            "commit-tree",
            &tree_sha,
            "-p",
            &parent_sha,
            "-m",
            "delayed branch plumbing commit",
        ],
    )
    .trim()
    .to_string();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let update_ref_trace = trace_dir.path().join("update-ref.trace2");
    raw_git_trace_to_file(
        &repo,
        &["update-ref", "refs/heads/feature", &commit_sha, &parent_sha],
        &update_ref_trace,
    );

    replay_trace_file_to_daemon(&repo, &update_ref_trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);

    assert_note_has_ai_for_file(&repo, &commit_sha, "delayed-branch-plumbing.txt");
    let mut feature_file = repo.filename("delayed-branch-plumbing.txt");
    feature_file.assert_lines_and_blame(lines!["branch ai".ai()]);
}

#[test]
fn test_update_ref_side_effect_waits_for_prior_open_trace_root_without_family() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    repo.git(&["checkout", "-b", "feature"])
        .expect("checkout feature should succeed");

    fs::write(
        repo.path().join("sequenced-branch-plumbing.txt"),
        "sequenced branch ai\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "sequenced-branch-plumbing.txt"])
        .unwrap();
    raw_untraced_git(&repo, &["add", "-A"]);
    repo.sync_daemon();

    let parent_sha = head_sha(&repo);
    let tree_sha = raw_untraced_git(&repo, &["write-tree"]).trim().to_string();
    let commit_sha = raw_untraced_git(
        &repo,
        &[
            "commit-tree",
            &tree_sha,
            "-p",
            &parent_sha,
            "-m",
            "sequenced branch plumbing commit",
        ],
    )
    .trim()
    .to_string();

    let unfinished_trace =
        open_unfinished_mutating_trace_root(&repo, "20260411T120000.000000-Punfinished-root");
    std::thread::sleep(Duration::from_millis(100));

    let session = new_daemon_test_sync_session_id();
    raw_traced_git_with_session(
        &repo,
        &["update-ref", "refs/heads/feature", &commit_sha, &parent_sha],
        &session,
    );

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        assert!(
            !daemon_completed_session(&repo, &session),
            "update-ref side effect completed while an earlier mutating trace root was still open without family metadata"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    drop(unfinished_trace);
    repo.sync_daemon_external_completion_sessions(&[session]);

    assert_note_has_ai_for_file(&repo, &commit_sha, "sequenced-branch-plumbing.txt");
    let mut feature_file = repo.filename("sequenced-branch-plumbing.txt");
    feature_file.assert_lines_and_blame(lines!["sequenced branch ai".ai()]);
}

#[test]
fn test_update_ref_stdin_head_with_new_content_preserves_attribution() {
    let repo = TestRepo::new();
    setup_initial_commit(&repo);

    fs::write(repo.path().join("stdin.txt"), "stdin ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "stdin.txt"])
        .unwrap();
    raw_untraced_git(&repo, &["add", "-A"]);

    let parent_sha = head_sha(&repo);
    let tree_sha = raw_untraced_git(&repo, &["write-tree"]).trim().to_string();
    let commit_sha = raw_untraced_git(
        &repo,
        &[
            "commit-tree",
            &tree_sha,
            "-p",
            &parent_sha,
            "-m",
            "stdin commit",
        ],
    )
    .trim()
    .to_string();

    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();
    raw_traced_git_stdin(
        &repo,
        &["update-ref", "--stdin"],
        &format!("update HEAD {} {}\n", commit_sha, parent_sha),
    );
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);

    assert_note_has_ai_for_file(&repo, &commit_sha, "stdin.txt");
}
