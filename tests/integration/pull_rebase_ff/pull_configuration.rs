use super::*;

// =============================================================================
// Pull --rebase with committed changes (the core bug fix)
// =============================================================================

#[test]
fn test_pull_rebase_preserves_committed_ai_authorship() {
    let setup = setup_divergent_pull_test();
    let local = setup.local;

    // Perform pull --rebase (committed local changes will be rebased onto upstream)
    local
        .git(&["pull", "--rebase"])
        .expect("pull --rebase should succeed");

    // Verify we got upstream changes
    assert!(
        local.read_file("upstream_change.txt").is_some(),
        "Should have upstream_change.txt after pull --rebase"
    );

    // The AI commit got a new SHA after rebase
    let new_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    assert_ne!(
        new_head, setup.local_ai_commit_sha,
        "HEAD should have a new SHA after rebase"
    );

    // Verify AI authorship is preserved on the rebased commit
    let mut ai_file = local.filename("ai_feature.txt");
    ai_file.assert_lines_and_blame(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
}

#[test]
fn test_rejected_push_failed_pull_then_pull_rebase_preserves_committed_ai_authorship() {
    let (local, upstream) = TestRepo::new_with_remote();

    let mut readme = local.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    local
        .stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");
    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push initial commit should succeed");

    let mut ai_file = local.filename("ai_feature.txt");
    ai_file.set_contents(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
    let local_ai_commit = local
        .stage_all_and_commit("add AI feature")
        .expect("AI feature commit should succeed");

    assert!(
        local
            .read_authorship_note(&local_ai_commit.commit_sha)
            .is_some(),
        "precondition: original local AI commit should have authorship note"
    );

    let branch = local.current_branch();
    let contributor_parent = tempfile::tempdir().expect("contributor temp dir");
    let contributor_path = contributor_parent.path().join("contributor");
    local
        .git_og(&[
            "clone",
            upstream
                .path()
                .to_str()
                .expect("upstream path should be utf-8"),
            contributor_path
                .to_str()
                .expect("contributor path should be utf-8"),
        ])
        .expect("clone contributor should succeed");
    let contributor =
        TestRepo::new_at_path_with_daemon_scope(&contributor_path, DaemonTestScope::NoDaemon);
    std::fs::write(
        contributor.path().join("upstream_change.txt"),
        "upstream content\n",
    )
    .expect("write upstream change");
    contributor.git_og(&["add", "."]).unwrap();
    contributor
        .git_og(&["commit", "-m", "upstream divergent commit"])
        .expect("upstream commit should succeed");
    contributor
        .git_og(&["push", "origin", &format!("HEAD:{}", branch)])
        .expect("push upstream divergence should succeed");

    assert!(
        local.git(&["push"]).is_err(),
        "push should be rejected because origin has diverged"
    );
    assert!(
        local.git(&["pull"]).is_err(),
        "plain pull should fail before an explicit reconciliation strategy"
    );

    local
        .git(&["pull", "--rebase"])
        .expect("pull --rebase should succeed");

    let rebased_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();
    assert_ne!(
        rebased_head, local_ai_commit.commit_sha,
        "HEAD should have a new SHA after rebase"
    );
    assert!(
        local.read_authorship_note(&rebased_head).is_some(),
        "rebased local AI commit should have authorship note"
    );

    ai_file.assert_lines_and_blame(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
}

#[test]
fn test_pull_rebase_via_git_config_preserves_committed_ai_authorship() {
    let setup = setup_divergent_pull_test();
    let local = setup.local;

    // Set git config to use rebase for pull (no --rebase flag needed)
    local
        .git(&["config", "pull.rebase", "true"])
        .expect("set pull.rebase should succeed");

    // Perform plain pull (should rebase due to config)
    local.git(&["pull"]).expect("pull should succeed");

    // Verify upstream changes arrived and commit SHA changed
    assert!(
        local.read_file("upstream_change.txt").is_some(),
        "Should have upstream_change.txt after pull"
    );

    let new_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    assert_ne!(
        new_head, setup.local_ai_commit_sha,
        "HEAD should have a new SHA after rebase"
    );

    // Verify AI authorship survived
    let mut ai_file = local.filename("ai_feature.txt");
    ai_file.assert_lines_and_blame(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
}

#[test]
fn test_pull_rebase_via_alias_preserves_committed_ai_authorship() {
    // Regression: `git up` where `up = pull --rebase`. Git expands the alias
    // before writing the reflog (label `pull --rebase ... (start)`), but the
    // daemon previously reconstructed the pull action from the literal alias
    // token `up`, so the span matcher never matched and the rebased AI commit's
    // authorship note was dropped. The invocation must expand to `pull
    // --rebase` so attribution migrates with the rebase.
    let setup = setup_divergent_pull_test();
    let local = setup.local;

    // Define an alias that expands to `pull --rebase`.
    local
        .git(&["config", "alias.up", "pull --rebase"])
        .expect("set alias.up should succeed");

    // Drive the rebase entirely through the alias (no explicit --rebase flag).
    local.git(&["up"]).expect("aliased pull should succeed");

    // Verify upstream changes arrived and the commit SHA changed (real rebase).
    assert!(
        local.read_file("upstream_change.txt").is_some(),
        "Should have upstream_change.txt after aliased pull --rebase"
    );

    let new_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    assert_ne!(
        new_head, setup.local_ai_commit_sha,
        "HEAD should have a new SHA after rebase"
    );

    // Verify AI authorship survived the alias-driven rebase.
    let mut ai_file = local.filename("ai_feature.txt");
    ai_file.assert_lines_and_blame(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
}

#[test]
fn test_pull_rebase_via_zero_arg_alias_and_git_config_preserves_committed_ai_authorship() {
    // Regression: `git up` where `up = pull` and `pull.rebase=true`. The alias
    // expands to `pull` with no explicit args, so the normalized invocation must
    // still keep `pull` visible instead of falling back to the raw alias token.
    let setup = setup_divergent_pull_test();
    let local = setup.local;

    local
        .git(&["config", "alias.up", "pull"])
        .expect("set alias.up should succeed");
    local
        .git(&["config", "pull.rebase", "true"])
        .expect("set pull.rebase should succeed");

    local.git(&["up"]).expect("aliased pull should succeed");

    assert!(
        local.read_file("upstream_change.txt").is_some(),
        "Should have upstream_change.txt after aliased config-driven pull --rebase"
    );

    let new_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    assert_ne!(
        new_head, setup.local_ai_commit_sha,
        "HEAD should have a new SHA after rebase"
    );

    let mut ai_file = local.filename("ai_feature.txt");
    ai_file.assert_lines_and_blame(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
}

#[test]
fn test_pull_rebase_autostash_via_git_config() {
    let setup = setup_pull_test();
    let local = setup.local;

    // Set git config to always use rebase and autostash for pull
    local
        .git(&["config", "pull.rebase", "true"])
        .expect("set pull.rebase should succeed");
    local
        .git(&["config", "rebase.autoStash", "true"])
        .expect("set rebase.autoStash should succeed");

    // Create local uncommitted AI changes
    let mut ai_file = local.filename("ai_config_test.txt");
    ai_file.set_contents(vec!["AI line via config".ai()]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Perform regular pull (should use rebase+autostash from config)
    local.git(&["pull"]).expect("pull should succeed");

    // Commit and verify AI attribution is preserved
    local
        .stage_all_and_commit("commit after config-based rebase pull")
        .expect("commit should succeed");

    ai_file.assert_lines_and_blame(vec!["AI line via config".ai()]);
}

#[test]
fn test_pull_rebase_preserves_authorship_when_range_diff_ignores_no_abbrev() {
    let setup = setup_divergent_pull_test_with_daemon_scope(DaemonTestScope::Dedicated);
    let mut local = setup.local;

    assert!(
        local
            .read_authorship_note(&setup.local_ai_commit_sha)
            .is_some(),
        "precondition: original local AI commit should have an authorship note"
    );

    // Route the daemon's git through a shim that simulates legacy Git
    // versions abbreviating range-diff output despite --no-abbrev.
    let shim_binary = env!("CARGO_BIN_EXE_git-ai-test-git-shim");
    let shim_path = local.test_home_path().join(if cfg!(windows) {
        "legacy-git-shim.exe"
    } else {
        "legacy-git-shim"
    });
    std::fs::copy(shim_binary, &shim_path).expect("legacy Git shim should be copied");
    let shim_path = shim_path.to_str().expect("shim path should be utf-8");
    let real_git = real_git_executable();
    local.patch_git_ai_config(|patch| patch.git_path = Some(shim_path.to_string()));
    local.restart_dedicated_daemon_with_env_for_test(&[
        ("GIT_AI_TEST_GIT_SHIM_TARGET", real_git),
        ("GIT_AI_TEST_GIT_SHIM_FALLBACK_TARGET", real_git),
        ("GIT_AI_TEST_GIT_SHIM_ABBREVIATE_RANGE_DIFF", "1"),
    ]);

    local
        .git(&["pull", "--rebase"])
        .expect("pull --rebase should succeed");

    let rebased_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();
    assert_ne!(rebased_head, setup.local_ai_commit_sha);
    assert!(
        local.read_authorship_note(&rebased_head).is_some(),
        "rebased local AI commit should retain its authorship note even when Git ignores --no-abbrev"
    );

    local.patch_git_ai_config(|patch| patch.git_path = Some(real_git.to_string()));
    let mut ai_file = local.filename("ai_feature.txt");
    ai_file.assert_committed_lines(crate::lines![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_pull_rebase_preserves_committed_ai_authorship,
    test_pull_rebase_via_git_config_preserves_committed_ai_authorship,
    test_pull_rebase_via_zero_arg_alias_and_git_config_preserves_committed_ai_authorship,
    test_pull_rebase_autostash_via_git_config,
);
