use super::{
    CommitStats, Duration, ExpectedLineExt, Instant, TestRepo, extract_json_object, fs,
    stats_while_restoring_authorship_note,
};

#[test]
fn test_stats_default_waits_for_recent_commit_authorship_note() {
    let repo = TestRepo::new();
    let mut file = repo.filename("recent-default.txt");
    file.set_contents(crate::lines!["AI line".ai()]);
    let commit = repo.stage_all_and_commit("recent AI commit").unwrap();
    let started = Instant::now();
    let output =
        stats_while_restoring_authorship_note(&repo, &commit.commit_sha, &["stats", "--json"]);

    assert!(
        started.elapsed() >= Duration::from_millis(100),
        "stats returned before the delayed note was restored"
    );
    assert!(
        output.contains("Waiting for git-ai to process this commit"),
        "interactive stats should show a waiting indicator, got:\n{output}"
    );
    let stats: CommitStats = serde_json::from_str(&extract_json_object(&output)).unwrap();
    assert_eq!(stats.ai_additions, 1);
    assert_eq!(stats.unknown_additions, 0);
}

#[test]
fn test_stats_single_rev_waits_for_recent_commit_authorship_note() {
    let repo = TestRepo::new();
    let mut file = repo.filename("recent-rev.txt");
    file.set_contents(crate::lines!["AI line".ai()]);
    let commit = repo.stage_all_and_commit("recent AI commit").unwrap();
    let started = Instant::now();
    let output = stats_while_restoring_authorship_note(
        &repo,
        &commit.commit_sha,
        &["stats", &commit.commit_sha, "--json"],
    );

    assert!(
        started.elapsed() >= Duration::from_millis(100),
        "stats <rev> returned before the delayed note was restored"
    );
    let stats: CommitStats = serde_json::from_str(&extract_json_object(&output)).unwrap();
    assert_eq!(stats.ai_additions, 1);
    assert_eq!(stats.unknown_additions, 0);
}

#[test]
fn test_stats_does_not_wait_when_collection_is_denied() {
    let mut repo = TestRepo::new_dedicated_daemon();
    repo.patch_git_ai_config(|patch| {
        patch.allowed_repositories = Some(vec![]);
    });

    fs::write(repo.path().join("denied.txt"), "untracked line\n").unwrap();
    repo.git_og(&["add", "denied.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "recent denied commit"])
        .unwrap();

    let output = repo
        .git_ai_with_env_without_pre_sync_for_test(
            &["stats", "--json"],
            &[("GIT_AI_TEST_FORCE_TTY", "1")],
        )
        .expect("stats should work without an authorship note");
    assert!(
        !output.contains("Waiting for git-ai to process this commit"),
        "stats must not wait for attribution that collection policy forbids:\n{output}"
    );
}

#[test]
fn test_stats_does_not_wait_for_old_commit_without_authorship_note() {
    let repo = TestRepo::new();
    fs::write(repo.path().join("old.txt"), "old line\n").unwrap();
    repo.git_og(&["add", "old.txt"]).unwrap();
    repo.git_og_with_env(
        &["commit", "-m", "old commit"],
        &[
            ("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z"),
            ("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z"),
        ],
    )
    .unwrap();

    let output = repo
        .git_ai_with_env_without_pre_sync_for_test(
            &["stats", "--json"],
            &[("GIT_AI_TEST_FORCE_TTY", "1")],
        )
        .expect("stats should work without an authorship note");
    assert!(
        !output.contains("Waiting for git-ai to process this commit"),
        "stats must not wait for an old commit:\n{output}"
    );
}

#[test]
fn test_stats_range_does_not_wait_for_missing_authorship_note() {
    let repo = TestRepo::new();
    fs::write(repo.path().join("range-wait.txt"), "first\n").unwrap();
    repo.git_og(&["add", "range-wait.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "first raw commit"]).unwrap();
    let first = repo.git_og(&["rev-parse", "HEAD"]).unwrap();

    fs::write(repo.path().join("range-wait.txt"), "first\nsecond\n").unwrap();
    repo.git_og(&["add", "range-wait.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "second raw commit"]).unwrap();
    let second = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    let range = format!("{}..{}", first.trim(), second.trim());

    let output = repo
        .git_ai_with_env_without_pre_sync_for_test(
            &["stats", &range, "--json"],
            &[("GIT_AI_TEST_FORCE_TTY", "1")],
        )
        .expect("stats range should work without authorship notes");
    assert!(
        !output.contains("Waiting for git-ai to process this commit"),
        "stats ranges must not use the single-commit wait path:\n{output}"
    );
}
