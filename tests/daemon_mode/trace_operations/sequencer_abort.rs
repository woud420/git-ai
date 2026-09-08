use super::super::*;

#[test]
#[serial]
fn daemon_pure_trace_socket_rebase_abort_emits_abort_event() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start(&repo);
    let trace_socket = daemon_trace_socket_path(&repo);
    let env = git_trace_env(&trace_socket);
    let env_refs = [(env[0].0, env[0].1.as_str()), (env[1].0, env[1].1.as_str())];
    let default_branch = repo.current_branch();
    let completion_baseline = repo.daemon_total_completion_count();
    let mut expected_top_level_completions = 0u64;

    fs::write(repo.path().join("rebase-conflict.txt"), "base\n").expect("failed to write base");
    traced_git_with_env(
        &repo,
        &["add", "rebase-conflict.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("add should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "base"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("base commit should succeed");

    traced_git_with_env(
        &repo,
        &["checkout", "-b", "feature"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("feature branch checkout should succeed");
    fs::write(repo.path().join("rebase-conflict.txt"), "feature\n")
        .expect("failed to write feature branch change");
    traced_git_with_env(
        &repo,
        &["add", "rebase-conflict.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("feature add should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "feature change"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("feature commit should succeed");

    traced_git_with_env(
        &repo,
        &["checkout", default_branch.as_str()],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("checkout default branch should succeed");
    fs::write(repo.path().join("rebase-conflict.txt"), "main\n")
        .expect("failed to write default branch change");
    traced_git_with_env(
        &repo,
        &["add", "rebase-conflict.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("default branch add should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "main change"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("default branch commit should succeed");

    traced_git_with_env(
        &repo,
        &["checkout", "feature"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("checkout feature should succeed");
    let rebase_conflict = traced_git_with_env(
        &repo,
        &["rebase", default_branch.as_str()],
        &env_refs,
        &mut expected_top_level_completions,
    );
    assert!(
        rebase_conflict.is_err(),
        "rebase should conflict for abort flow coverage"
    );
    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );
    traced_git_with_env(
        &repo,
        &["rebase", "--abort"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("rebase abort should succeed");

    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );
}

#[test]
#[serial]
fn daemon_pure_trace_socket_cherry_pick_abort_emits_abort_event() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start(&repo);
    let trace_socket = daemon_trace_socket_path(&repo);
    let env = git_trace_env(&trace_socket);
    let env_refs = [(env[0].0, env[0].1.as_str()), (env[1].0, env[1].1.as_str())];
    let default_branch = repo.current_branch();
    let completion_baseline = repo.daemon_total_completion_count();
    let mut expected_top_level_completions = 0u64;

    fs::write(repo.path().join("cherry-conflict.txt"), "base\n").expect("failed to write base");
    traced_git_with_env(
        &repo,
        &["add", "cherry-conflict.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("add should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "base"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("base commit should succeed");

    traced_git_with_env(
        &repo,
        &["checkout", "-b", "topic"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("topic branch checkout should succeed");
    fs::write(repo.path().join("cherry-conflict.txt"), "topic\n")
        .expect("failed to write topic branch change");
    traced_git_with_env(
        &repo,
        &["add", "cherry-conflict.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("topic add should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "topic change"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("topic commit should succeed");
    let topic_sha = repo
        .git(&["rev-parse", "topic"])
        .expect("topic rev-parse should succeed")
        .trim()
        .to_string();

    traced_git_with_env(
        &repo,
        &["checkout", default_branch.as_str()],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("checkout default branch should succeed");
    fs::write(repo.path().join("cherry-conflict.txt"), "main\n")
        .expect("failed to write default branch conflicting change");
    traced_git_with_env(
        &repo,
        &["add", "cherry-conflict.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("default branch add should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "main change"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("default branch commit should succeed");

    let cherry_pick_conflict = traced_git_with_env(
        &repo,
        &["cherry-pick", topic_sha.as_str()],
        &env_refs,
        &mut expected_top_level_completions,
    );
    assert!(
        cherry_pick_conflict.is_err(),
        "cherry-pick should conflict for abort flow coverage"
    );
    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );
    traced_git_with_env(
        &repo,
        &["cherry-pick", "--abort"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("cherry-pick abort should succeed");

    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );
}

#[test]
#[serial]
fn daemon_pure_trace_socket_stash_main_ops_emit_stash_events() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start(&repo);
    let trace_socket = daemon_trace_socket_path(&repo);
    let env = git_trace_env(&trace_socket);
    let env_refs = [(env[0].0, env[0].1.as_str()), (env[1].0, env[1].1.as_str())];
    let completion_baseline = repo.daemon_total_completion_count();
    let mut expected_top_level_completions = 0u64;

    fs::write(repo.path().join("stash-case.txt"), "base\n").expect("failed to write base");
    traced_git_with_env(
        &repo,
        &["add", "stash-case.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("base add should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "base"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("base commit should succeed");

    fs::write(repo.path().join("stash-case.txt"), "base\nchange one\n")
        .expect("failed to write stash content");
    traced_git_with_env(
        &repo,
        &["stash", "push", "-m", "save one"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("stash push should succeed");
    // `git stash list` is readonly — the daemon's readonly fast-path drops it
    // before it reaches the ingest queue, so we run it without incrementing
    // expected_top_level_completions and do not expect it in the rewrite log.
    repo.git_og_with_env(&["stash", "list"], &env_refs)
        .expect("stash list should succeed");
    traced_git_with_env(
        &repo,
        &["stash", "apply", "stash@{0}"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("stash apply should succeed");

    traced_git_with_env(
        &repo,
        &["reset", "--hard", "HEAD"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("reset hard should succeed");
    traced_git_with_env(
        &repo,
        &["stash", "pop", "stash@{0}"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("stash pop should succeed");

    traced_git_with_env(
        &repo,
        &["add", "stash-case.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("add before commit should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "stash pop result"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("commit after stash pop should succeed");

    fs::write(repo.path().join("stash-case.txt"), "base\nchange two\n")
        .expect("failed to write second stash content");
    traced_git_with_env(
        &repo,
        &["stash", "push", "-m", "save two"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("second stash push should succeed");
    traced_git_with_env(
        &repo,
        &["stash", "drop", "stash@{0}"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("stash drop should succeed");

    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );
}
