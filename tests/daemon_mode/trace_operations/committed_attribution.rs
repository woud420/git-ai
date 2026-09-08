use super::super::*;

#[test]
#[serial]
fn daemon_pure_trace_socket_checkpoint_stage_checkpoint_two_commits_preserve_ai_lines() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start(&repo);
    let trace_socket = daemon_trace_socket_path(&repo);
    let env = git_trace_env(&trace_socket);
    let env_refs = [(env[0].0, env[0].1.as_str()), (env[1].0, env[1].1.as_str())];
    let file_rel = "daemon-two-ai-lines.txt";
    let file_path = repo.path().join(file_rel);
    let completion_baseline = repo.daemon_total_completion_count();
    let mut expected_top_level_completions = 0u64;

    fs::write(&file_path, "base\n").expect("failed to seed base file");
    traced_git_with_env(
        &repo,
        &["add", file_rel],
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

    {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&file_path)
            .expect("failed to open file for first append");
        writeln!(f, "test").expect("failed to append first ai line");
    }
    repo.git_ai_with_env(
        &["checkpoint", "mock_ai", file_rel],
        &[("GIT_AI_DAEMON_CHECKPOINT_DELEGATE", "true")],
    )
    .expect("first delegated ai checkpoint should succeed");
    expected_top_level_completions += 1;
    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );

    traced_git_with_env(
        &repo,
        &["add", "."],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("staging first ai line should succeed");

    {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&file_path)
            .expect("failed to open file for second append");
        writeln!(f, "test1").expect("failed to append second ai line");
    }
    repo.git_ai_with_env(
        &["checkpoint", "mock_ai", file_rel],
        &[("GIT_AI_DAEMON_CHECKPOINT_DELEGATE", "true")],
    )
    .expect("second delegated ai checkpoint should succeed");
    expected_top_level_completions += 1;
    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );

    traced_git_with_env(
        &repo,
        &["commit", "-m", "first ai line"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("first commit should succeed");
    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );

    traced_git_with_env(
        &repo,
        &["add", "."],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("staging second ai line should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "second ai line"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("second commit should succeed");
    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );

    let mut file = repo.filename(file_rel);
    file.assert_lines_and_blame(lines!["base", "test".ai(), "test1".ai()]);
}

#[test]
#[serial]
fn daemon_pure_trace_socket_checkpoint_stage_checkpoint_non_adjacent_hunks_survive_split_commits() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start(&repo);
    let trace_socket = daemon_trace_socket_path(&repo);
    let env = git_trace_env(&trace_socket);
    let env_refs = [(env[0].0, env[0].1.as_str()), (env[1].0, env[1].1.as_str())];
    let file_rel = "daemon-non-adjacent.md";
    let file_path = repo.path().join(file_rel);
    let completion_baseline = repo.daemon_total_completion_count();
    let mut expected_top_level_completions = 0u64;

    let initial = "\
Top line

**Section Alpha**
alpha body

middle line 1
middle line 2

**Section Omega**
omega body
";
    fs::write(&file_path, initial).expect("failed to write initial content");
    traced_git_with_env(
        &repo,
        &["add", file_rel],
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

    let first_ai_hunk = "\
Top line

### Section Alpha
alpha body

middle line 1
middle line 2

**Section Omega**
omega body
";
    fs::write(&file_path, first_ai_hunk).expect("failed to write first hunk content");
    repo.git_ai_with_env(
        &["checkpoint", "mock_ai", file_rel],
        &[("GIT_AI_DAEMON_CHECKPOINT_DELEGATE", "true")],
    )
    .expect("first delegated checkpoint should succeed");
    expected_top_level_completions += 1;
    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );

    traced_git_with_env(
        &repo,
        &["add", "."],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("staging first hunk should succeed");

    let both_hunks = "\
Top line

### Section Alpha
alpha body

middle line 1
middle line 2

### Section Omega
omega body
";
    fs::write(&file_path, both_hunks).expect("failed to write both hunks content");
    repo.git_ai_with_env(
        &["checkpoint", "mock_ai", file_rel],
        &[("GIT_AI_DAEMON_CHECKPOINT_DELEGATE", "true")],
    )
    .expect("second delegated checkpoint should succeed");
    expected_top_level_completions += 1;
    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );

    traced_git_with_env(
        &repo,
        &["commit", "-m", "commit first staged hunk"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("first split commit should succeed");
    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );

    traced_git_with_env(
        &repo,
        &["add", "."],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("staging remaining hunk should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "commit second hunk"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("second split commit should succeed");
    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );

    let mut file = repo.filename(file_rel);
    file.assert_lines_and_blame(lines![
        "Top line",
        "".human(),
        "### Section Alpha".ai(),
        "alpha body",
        "".human(),
        "middle line 1",
        "middle line 2",
        "".human(),
        "### Section Omega".ai(),
        "omega body",
    ]);
}

#[test]
#[serial]
fn daemon_pure_trace_socket_write_mode_applies_amend_rewrite() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let _daemon = DaemonGuard::start(&repo);
    let trace_socket = daemon_trace_socket_path(&repo);
    let env = git_trace_env(&trace_socket);
    let env_refs = [(env[0].0, env[0].1.as_str()), (env[1].0, env[1].1.as_str())];
    let completion_baseline = repo.daemon_total_completion_count();
    let mut expected_top_level_completions = 0u64;

    fs::write(repo.path().join("pure-trace.txt"), "line 1\n").expect("failed to write file");
    traced_git_with_env(
        &repo,
        &["add", "pure-trace.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("add should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "-m", "initial"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("commit should succeed");

    fs::write(repo.path().join("pure-trace.txt"), "line 1\nline 2\n")
        .expect("failed to update file");
    traced_git_with_env(
        &repo,
        &["add", "pure-trace.txt"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("add before amend should succeed");
    traced_git_with_env(
        &repo,
        &["commit", "--amend", "-m", "initial amended"],
        &env_refs,
        &mut expected_top_level_completions,
    )
    .expect("amend should succeed");

    wait_for_expected_top_level_completions(
        &repo,
        completion_baseline,
        expected_top_level_completions,
    );
}
