use super::{
    BTreeSet, DaemonTestScope, ExpectedLineExt, TestRepo, insert_bash_recovery_call_covering_now,
    isolated_bash_history_db_path, setup_pull_test,
};

// =============================================================================
// Fast-forward pull tests
// =============================================================================

#[test]
fn test_fast_forward_pull_preserves_ai_attribution() {
    let setup = setup_pull_test();
    let local = setup.local;

    // Create local AI changes (uncommitted)
    let mut ai_file = local.filename("ai_work.txt");
    ai_file.set_contents(vec!["AI generated line 1".ai(), "AI generated line 2".ai()]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Configure git pull behavior for Git 2.52.0+ compatibility
    local
        .git(&["config", "pull.rebase", "false"])
        .expect("config should succeed");
    local
        .git(&["config", "pull.ff", "only"])
        .expect("config should succeed");

    // Perform fast-forward pull
    local.git(&["pull"]).expect("pull should succeed");

    // Commit and verify AI attribution is preserved through the ff pull
    local
        .stage_all_and_commit("commit after pull")
        .expect("commit should succeed");
    ai_file.assert_lines_and_blame(vec!["AI generated line 1".ai(), "AI generated line 2".ai()]);
}

#[test]
fn test_fast_forward_pull_without_local_changes() {
    let setup = setup_pull_test();
    let local = setup.local;

    // Configure git pull behavior
    local
        .git(&["config", "pull.ff", "only"])
        .expect("config should succeed");

    // No local changes - just a clean fast-forward pull
    local.git(&["pull"]).expect("pull should succeed");

    // Verify we got the upstream changes
    assert!(
        local.read_file("upstream_file.txt").is_some(),
        "Should have upstream_file.txt after pull"
    );

    // Verify HEAD is at the expected upstream commit
    let head = local.git(&["rev-parse", "HEAD"]).unwrap();
    assert_eq!(
        head.trim(),
        setup.upstream_sha,
        "HEAD should be at upstream commit"
    );
}

#[test]
fn test_fast_forward_update_ref_bounds_recovery_to_new_tip_parent() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let local = TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH",
        bash_db_path.as_str(),
    )]);
    let upstream_dir = tempfile::tempdir().expect("upstream temp dir");
    let upstream_path = upstream_dir.path().join("upstream.git");
    let upstream = upstream_path.to_string_lossy().to_string();

    local
        .git_og(&["init", "--bare", &upstream])
        .expect("bare upstream init should succeed");
    local
        .git(&["remote", "add", "origin", &upstream])
        .expect("remote add should succeed");

    std::fs::write(local.path().join("base.txt"), "base\n").expect("write base");
    let old_tip = local
        .stage_all_and_commit("old tip")
        .expect("old tip commit should succeed")
        .commit_sha;
    let branch = local.current_branch();
    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push old tip should succeed");
    local
        .git_og(&[
            "--git-dir",
            &upstream,
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{branch}"),
        ])
        .expect("set upstream HEAD should succeed");

    // Seed a working log at the old local tip. The daemon fast-forward
    // update-ref path only finalizes attribution when such a log exists.
    std::fs::write(local.path().join("local_draft.txt"), "local draft\n")
        .expect("write local draft");
    local
        .git_ai(&["checkpoint", "mock_ai", "local_draft.txt"])
        .expect("local checkpoint should succeed");

    let contributor_dir = tempfile::tempdir().expect("contributor temp dir");
    let contributor_path = contributor_dir.path().join("contributor");
    local
        .git_og(&[
            "clone",
            &upstream,
            contributor_path
                .to_str()
                .expect("contributor path should be utf-8"),
        ])
        .expect("contributor clone should succeed");
    let contributor =
        TestRepo::new_at_path_with_daemon_scope(&contributor_path, DaemonTestScope::NoDaemon);

    std::fs::write(
        contributor.path().join("pulled_early_1.txt"),
        "pulled early 1\n",
    )
    .expect("write pulled early 1");
    contributor.git_og(&["add", "-A"]).unwrap();
    contributor
        .git_og(&["commit", "-m", "pulled early 1"])
        .expect("commit pulled early 1 should succeed");
    std::fs::write(
        contributor.path().join("pulled_early_2.txt"),
        "pulled early 2\n",
    )
    .expect("write pulled early 2");
    contributor.git_og(&["add", "-A"]).unwrap();
    contributor
        .git_og(&["commit", "-m", "pulled early 2"])
        .expect("commit pulled early 2 should succeed");
    std::fs::write(contributor.path().join("final_tip.txt"), "final tip\n")
        .expect("write final tip");
    contributor.git_og(&["add", "-A"]).unwrap();
    contributor
        .git_og(&["commit", "-m", "final tip"])
        .expect("commit final tip should succeed");
    contributor
        .git_og(&["push", "origin", &format!("HEAD:{branch}")])
        .expect("push contributor range should succeed");

    local
        .git(&["fetch", "origin", &branch])
        .expect("fetch contributor range should succeed");
    let new_tip = local
        .git(&["rev-parse", "FETCH_HEAD"])
        .expect("rev-parse FETCH_HEAD should succeed")
        .trim()
        .to_string();
    let contributor_final = contributor
        .git_og(&["rev-parse", "HEAD"])
        .expect("rev-parse contributor HEAD should succeed")
        .trim()
        .to_string();
    assert_eq!(new_tip, contributor_final);
    assert_ne!(old_tip, new_tip);

    // `git update-ref` moves the ref but does not update the worktree. Put the
    // files in the worktree so timestamp-based recovery can run deterministically
    // and prove which committed hunks it was allowed to inspect.
    std::fs::write(local.path().join("pulled_early_1.txt"), "pulled early 1\n")
        .expect("write local pulled early 1");
    std::fs::write(local.path().join("pulled_early_2.txt"), "pulled early 2\n")
        .expect("write local pulled early 2");
    std::fs::write(local.path().join("final_tip.txt"), "final tip\n")
        .expect("write local final tip");

    insert_bash_recovery_call_covering_now(&bash_db_path, &local);
    local
        .git(&[
            "update-ref",
            &format!("refs/heads/{branch}"),
            &new_tip,
            &old_tip,
        ])
        .expect("fast-forward update-ref should succeed");

    let new_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse HEAD should succeed")
        .trim()
        .to_string();
    assert_eq!(new_head, new_tip);

    let log = local.require_authorship_log(&new_head);
    let attested_files: BTreeSet<String> = log
        .attestations
        .iter()
        .map(|attestation| attestation.file_path.clone())
        .collect();

    assert!(
        attested_files.contains("final_tip.txt"),
        "recovery should still see the finalized tip commit"
    );
    assert!(
        !attested_files.contains("pulled_early_1.txt"),
        "recovery must not diff the whole old_tip..new_head range"
    );
    assert!(
        !attested_files.contains("pulled_early_2.txt"),
        "recovery must not attribute earlier pulled commits to the local session"
    );
}

crate::reuse_tests_in_worktree!(
    test_fast_forward_pull_preserves_ai_attribution,
    test_fast_forward_pull_without_local_changes,
);
