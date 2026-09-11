use super::*;

#[test]
fn test_diff_ignores_repo_external_diff_helper_but_proxy_uses_it() {
    let repo = TestRepo::new();

    let mut file = repo.filename("README.md");
    file.set_contents(crate::lines!["line one".human()]);
    repo.stage_all_and_commit("initial").unwrap();

    file.set_contents(crate::lines!["line one".human(), "line two".ai()]);
    repo.stage_all_and_commit("second").unwrap();

    let marker =
        configure_repo_external_diff_helper(&repo, "EXTERNAL_DIFF_MARKER", "ext-diff-helper.sh");

    let proxied_diff = repo
        .git(&["diff", "HEAD^", "HEAD"])
        .expect("proxied git diff should succeed");
    assert!(
        proxied_diff.contains(&marker),
        "proxied git diff should honor diff.external helper output, got:\n{}",
        proxied_diff
    );

    let git_ai_diff = repo
        .git_ai(&["diff", "HEAD"])
        .expect("git-ai diff should succeed");
    assert!(
        !git_ai_diff.contains(&marker),
        "git-ai diff should not use external diff helper output, got:\n{}",
        git_ai_diff
    );
    assert!(
        git_ai_diff.contains("diff --git"),
        "git-ai diff should emit standard unified diff output, got:\n{}",
        git_ai_diff
    );
    assert!(
        git_ai_diff.contains("@@"),
        "git-ai diff should include hunk headers, got:\n{}",
        git_ai_diff
    );
}

#[test]
fn test_diff_parsing_is_stable_under_hostile_diff_config() {
    let repo = TestRepo::new();

    let mut file = repo.filename("README.md");
    file.set_contents(crate::lines!["line one".human()]);
    repo.stage_all_and_commit("initial").unwrap();

    file.set_contents(crate::lines![
        "line one".human(),
        "line two".ai(),
        "line three".ai()
    ]);
    repo.stage_all_and_commit("second").unwrap();

    configure_hostile_diff_settings(&repo);

    let git_ai_diff = repo
        .git_ai(&["diff", "HEAD"])
        .expect("git-ai diff should succeed");
    assert!(git_ai_diff.contains("diff --git"));
    assert!(git_ai_diff.contains("@@"));
    assert!(git_ai_diff.contains("+line two"));
    assert!(git_ai_diff.contains("+line three"));
}

#[test]
fn test_checkpoint_and_commit_ignore_repo_external_diff_helper() {
    let repo = TestRepo::new();

    let file_path = repo.path().join("tracked.txt");
    std::fs::write(&file_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "tracked.txt"])
        .unwrap();
    let mut file = repo.filename("tracked.txt");
    repo.stage_all_and_commit("initial").unwrap();

    file.set_contents(crate::lines!["base".human(), "added by ai".ai()]);
    let marker =
        configure_repo_external_diff_helper(&repo, "EXTERNAL_DIFF_MARKER", "ext-diff-helper.sh");
    let proxied_diff = repo
        .git(&["diff", "HEAD"])
        .expect("proxied git diff should succeed");
    assert!(
        proxied_diff.contains(&marker),
        "sanity check: external diff helper should be active for proxied git diff"
    );

    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed with external diff configured");
    repo.stage_all_and_commit("ai commit").unwrap();

    file.assert_lines_and_blame(crate::lines!["base".human(), "added by ai".ai()]);
}

#[test]
fn test_diff_ignores_git_external_diff_env_but_proxy_uses_it() {
    let repo = TestRepo::new();

    let mut file = repo.filename("env-diff.txt");
    file.set_contents(crate::lines!["before".human()]);
    repo.stage_all_and_commit("initial").unwrap();

    file.set_contents(crate::lines!["before".human(), "after".ai()]);
    repo.stage_all_and_commit("second").unwrap();

    let marker = "ENV_EXTERNAL_DIFF_MARKER";
    let helper_path = create_external_diff_helper_script(&repo, marker);
    let helper_path_str = helper_path
        .to_str()
        .expect("helper path must be valid UTF-8")
        .replace('\\', "/")
        .to_string();

    let proxied = repo
        .git_with_env(
            &["diff", "HEAD^", "HEAD"],
            &[("GIT_EXTERNAL_DIFF", helper_path_str.as_str())],
            None,
        )
        .expect("proxied git diff should succeed");
    assert!(
        proxied.contains(marker),
        "proxied git diff should honor GIT_EXTERNAL_DIFF, got:\n{}",
        proxied
    );

    let ai_diff = repo
        .git_ai_with_env(
            &["diff", "HEAD"],
            &[("GIT_EXTERNAL_DIFF", helper_path_str.as_str())],
        )
        .expect("git-ai diff should succeed with GIT_EXTERNAL_DIFF set");
    assert!(
        !ai_diff.contains(marker),
        "git-ai diff should ignore GIT_EXTERNAL_DIFF for internal diff calls, got:\n{}",
        ai_diff
    );
    assert!(
        ai_diff.contains("diff --git"),
        "git-ai diff should still emit normal unified diff output, got:\n{}",
        ai_diff
    );
}

#[test]
fn test_diff_ignores_git_diff_opts_env_for_internal_diff() {
    let repo = TestRepo::new();

    let mut file = repo.filename("env-diff-opts.txt");
    file.set_contents(crate::lines![
        "line 1".human(),
        "line 2".human(),
        "line 3".human(),
        "line 4".human(),
        "line 5".human()
    ]);
    repo.stage_all_and_commit("initial").unwrap();

    file.set_contents(crate::lines![
        "line 1".human(),
        "line 2".human(),
        "line 3 changed".ai(),
        "line 4".human(),
        "line 5".human()
    ]);
    let commit = repo.stage_all_and_commit("change middle").unwrap();

    // Proxied git should honor this env var and output 0 context lines.
    let proxied = repo
        .git_with_env(
            &[
                "diff",
                &format!("{}^", commit.commit_sha),
                &commit.commit_sha,
            ],
            &[("GIT_DIFF_OPTS", "--unified=0")],
            None,
        )
        .expect("proxied git diff should succeed");
    let proxied_context_count = proxied
        .lines()
        .filter(|l| l.starts_with(' ') && !l.starts_with("  "))
        .count();
    assert_eq!(
        proxied_context_count, 0,
        "proxied git diff should honor GIT_DIFF_OPTS=--unified=0, got:\n{}",
        proxied
    );

    // git-ai diff should ignore GIT_DIFF_OPTS and keep normal context behavior.
    let ai_diff = repo
        .git_ai_with_env(
            &["diff", &commit.commit_sha],
            &[("GIT_DIFF_OPTS", "--unified=0")],
        )
        .expect("git-ai diff should succeed with GIT_DIFF_OPTS set");
    let ai_context_count = ai_diff
        .lines()
        .filter(|l| l.starts_with(' ') && !l.starts_with("  "))
        .count();
    assert!(
        ai_context_count >= 2,
        "git-ai diff should ignore GIT_DIFF_OPTS and preserve context lines, got:\n{}",
        ai_diff
    );
}

#[test]
fn test_diff_respects_effective_ignore_patterns() {
    let repo = TestRepo::new();
    let ignore_file_path = repo.path().join(".git-ai-ignore");
    fs::write(&ignore_file_path, "ignored/**\n").expect("should write .git-ai-ignore");

    let mut visible = repo.filename("src/visible.txt");
    let mut ignored = repo.filename("ignored/secret.txt");
    visible.set_contents(crate::lines!["base visible".human()]);
    ignored.set_contents(crate::lines!["base secret".ai()]);
    repo.stage_all_and_commit("Initial with ignored file")
        .unwrap();

    visible.set_contents(crate::lines!["base visible".human(), "new visible".ai()]);
    ignored.set_contents(crate::lines!["base secret".ai(), "new secret".ai()]);
    let change_commit = repo
        .stage_all_and_commit("Change visible and ignored")
        .unwrap();

    let terminal_output = repo
        .git_ai(&["diff", &change_commit.commit_sha])
        .expect("git-ai diff should succeed");
    assert!(
        terminal_output.contains("src/visible.txt"),
        "visible file should be present in diff output"
    );
    assert!(
        !terminal_output.contains("ignored/secret.txt"),
        "ignored file should be filtered from diff output"
    );

    let json_output = repo
        .git_ai(&["diff", &change_commit.commit_sha, "--json"])
        .expect("git-ai diff --json should succeed");
    let json: Value = serde_json::from_str(&json_output).expect("diff JSON should parse");
    assert!(json["files"].get("src/visible.txt").is_some());
    assert!(json["files"].get("ignored/secret.txt").is_none());

    let hunks = json["hunks"].as_array().expect("hunks should be an array");
    assert!(hunks.iter().all(|hunk| {
        hunk.get("file_path")
            .and_then(|value| value.as_str())
            .map(|file| file == "src/visible.txt")
            .unwrap_or(false)
    }));
}

crate::reuse_tests_in_worktree!(
    test_diff_ignores_repo_external_diff_helper_but_proxy_uses_it,
    test_diff_parsing_is_stable_under_hostile_diff_config,
    test_checkpoint_and_commit_ignore_repo_external_diff_helper,
    test_diff_ignores_git_external_diff_env_but_proxy_uses_it,
    test_diff_ignores_git_diff_opts_env_for_internal_diff,
    test_diff_respects_effective_ignore_patterns,
);
