use super::{
    CiContext, CiEvent, CiRunResult, ExpectedLineExt, GitAiRepository, TestRepo, add_self_origin,
    fs, read_authorship_v3, read_note,
};

/// Test that CI preserves fork notes for a merge commit (non-squash, non-rebase).
///
/// For merge commits from forks, the merged commits keep their original SHAs.
/// The CI should fetch notes from the fork and push them to origin.
#[test]
fn test_ci_fork_merge_commit() {
    // Setup: create "upstream" repo with initial commit
    let upstream = TestRepo::new();
    let mut file = upstream.filename("feature.js");

    file.set_contents(lines!["// Original code", "function original() {}"]);
    let base_commit = upstream.stage_all_and_commit("Initial commit").unwrap();
    upstream.git(&["branch", "-M", "main"]).unwrap();
    add_self_origin(&upstream);

    // Create "fork" as separate repo with shared history
    let fork = TestRepo::new();
    let upstream_path = upstream.path().to_str().unwrap().to_string();
    fork.git_og(&["remote", "add", "upstream", &upstream_path])
        .unwrap();
    fork.git_og(&["fetch", "upstream"]).unwrap();
    fork.git_og(&["checkout", "-b", "main", "upstream/main"])
        .unwrap();

    // Fork contributor adds AI code
    let mut fork_file = fork.filename("feature.js");
    fork_file.set_contents(lines![
        "// Original code",
        "function original() {}",
        "// AI feature from fork".ai(),
        "function forkFeature() {".ai(),
        "  return true;".ai(),
        "}".ai()
    ]);
    let fork_commit = fork.stage_all_and_commit("Add AI feature in fork").unwrap();
    let fork_head_sha = fork_commit.commit_sha.clone();

    // Verify fork has authorship notes
    let fork_repo = GitAiRepository::find_repository_in_path(fork.path().to_str().unwrap())
        .expect("Failed to find fork repository");
    assert!(
        read_authorship_v3(&fork_repo, &fork_head_sha).is_ok(),
        "Fork commit should have authorship notes"
    );

    // Fetch fork commits and create merge commit in upstream
    upstream
        .git_og(&["remote", "add", "fork", fork.path().to_str().unwrap()])
        .unwrap();
    upstream
        .git_og(&["fetch", "fork", "main:refs/fork/main"])
        .unwrap();

    // Create a merge commit (--no-ff ensures a merge commit is created)
    upstream
        .git_og(&[
            "merge",
            "--no-ff",
            "refs/fork/main",
            "-m",
            "Merge fork PR (#1)",
        ])
        .unwrap();
    let merge_sha = upstream
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Run CI context with fork_clone_url
    let upstream_repo = GitAiRepository::find_repository_in_path(upstream.path().to_str().unwrap())
        .expect("Failed to find upstream repository");

    let fork_url = fork.path().to_str().unwrap().to_string();

    let ci_context = CiContext::with_repository(
        upstream_repo,
        CiEvent::Merge {
            merge_commit_sha: merge_sha.clone(),
            head_ref: "main".to_string(),
            head_sha: fork_head_sha.clone(),
            base_ref: "main".to_string(),
            base_sha: base_commit.commit_sha.clone(),
            fork_clone_url: Some(fork_url),
        },
    );

    let result = ci_context.run().unwrap();

    // For merge commits from forks, notes should be preserved (not rewritten)
    assert!(
        matches!(result, CiRunResult::ForkNotesPreserved),
        "Expected ForkNotesPreserved for fork merge commit, got {:?}",
        result
    );

    // Verify the fork commit's authorship is accessible in upstream
    let upstream_repo2 =
        GitAiRepository::find_repository_in_path(upstream.path().to_str().unwrap())
            .expect("Failed to find upstream repository");
    let authorship_log = read_authorship_v3(&upstream_repo2, &fork_head_sha);
    assert!(
        authorship_log.is_ok(),
        "Fork commit's authorship should be accessible in upstream after CI run"
    );
}

#[test]
fn test_ci_fork_merge_commit_preserves_non_head_pr_notes_when_base_sha_is_post_merge() {
    let upstream = TestRepo::new();
    let mut file = upstream.filename("feature.js");

    file.set_contents(lines!["// Original code", "function original() {}"]);
    upstream.stage_all_and_commit("Initial commit").unwrap();
    upstream.git(&["branch", "-M", "main"]).unwrap();
    add_self_origin(&upstream);

    let fork = TestRepo::new();
    let upstream_path = upstream.path().to_str().unwrap().to_string();
    fork.git_og(&["remote", "add", "upstream", &upstream_path])
        .unwrap();
    fork.git_og(&["fetch", "upstream"]).unwrap();
    fork.git_og(&["checkout", "-b", "main", "upstream/main"])
        .unwrap();

    let mut fork_file = fork.filename("feature.js");
    fork_file.set_contents(lines![
        "// Original code",
        "function original() {}",
        "// AI feature from fork".ai(),
        "function forkFeature() {".ai(),
        "  return true;".ai(),
        "}".ai()
    ]);
    let ai_commit = fork.stage_all_and_commit("Add AI feature in fork").unwrap();
    let ai_commit_sha = ai_commit.commit_sha.clone();

    fs::write(
        fork.path().join("feature.js"),
        "\
// Original code
function original() {}
// AI feature from fork
function forkFeature() {
  return true;
}
// Human follow-up
",
    )
    .unwrap();
    fork.git_og(&["add", "-A"]).unwrap();
    fork.git_og(&["commit", "-m", "Human follow-up in fork"])
        .unwrap();
    let fork_head_sha = fork
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let fork_repo = GitAiRepository::find_repository_in_path(fork.path().to_str().unwrap())
        .expect("Failed to find fork repository");
    assert!(
        read_authorship_v3(&fork_repo, &ai_commit_sha).is_ok(),
        "first fork commit should have authorship notes"
    );
    assert!(
        read_note(&fork_repo, &fork_head_sha).is_none(),
        "raw git head commit should not have an authorship note"
    );

    upstream
        .git_og(&["remote", "add", "fork", fork.path().to_str().unwrap()])
        .unwrap();
    upstream
        .git_og(&["fetch", "fork", "main:refs/fork/main"])
        .unwrap();
    upstream
        .git_og(&[
            "merge",
            "--no-ff",
            "refs/fork/main",
            "-m",
            "Merge fork PR (#2)",
        ])
        .unwrap();
    let merge_sha = upstream
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let upstream_repo = GitAiRepository::find_repository_in_path(upstream.path().to_str().unwrap())
        .expect("Failed to find upstream repository");
    let ci_context = CiContext::with_repository(
        upstream_repo,
        CiEvent::Merge {
            merge_commit_sha: merge_sha.clone(),
            head_ref: "main".to_string(),
            head_sha: fork_head_sha.clone(),
            base_ref: "main".to_string(),
            // For merge commits, the first parent is the target-side boundary
            // even if a caller passes a post-merge base ref/SHA.
            base_sha: merge_sha,
            fork_clone_url: Some(fork.path().to_str().unwrap().to_string()),
        },
    );

    let result = ci_context.run().unwrap();
    assert!(
        matches!(result, CiRunResult::ForkNotesPreserved),
        "Expected ForkNotesPreserved for fork merge commit, got {:?}",
        result
    );

    let upstream_repo_after =
        GitAiRepository::find_repository_in_path(upstream.path().to_str().unwrap())
            .expect("Failed to find upstream repository");
    assert!(
        read_authorship_v3(&upstream_repo_after, &ai_commit_sha).is_ok(),
        "non-head PR commit authorship should be preserved"
    );
    assert!(
        read_note(&upstream_repo_after, &fork_head_sha).is_none(),
        "head commit should remain without a note"
    );
}

/// Test that non-fork PRs (fork_clone_url = None) still work as before.
/// Merge commits without fork_clone_url should still be skipped.
#[test]
fn test_ci_non_fork_merge_commit_still_skipped() {
    let upstream = TestRepo::new();
    let mut file = upstream.filename("feature.js");

    file.set_contents(lines!["// Original code"]);
    let base_commit = upstream.stage_all_and_commit("Initial commit").unwrap();
    upstream.git(&["branch", "-M", "main"]).unwrap();
    add_self_origin(&upstream);

    // Create feature branch (same repo, not a fork)
    upstream.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = upstream.filename("feature.js");
    feature_file.set_contents(lines![
        "// Original code",
        "// Feature addition".ai(),
        "function feature() {}".ai()
    ]);
    let feature_commit = upstream.stage_all_and_commit("Add feature").unwrap();
    let feature_sha = feature_commit.commit_sha.clone();

    // Merge with --no-ff to create merge commit
    upstream.git(&["checkout", "main"]).unwrap();
    upstream
        .git_og(&["merge", "--no-ff", "feature", "-m", "Merge feature"])
        .unwrap();
    let merge_sha = upstream
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let upstream_repo = GitAiRepository::find_repository_in_path(upstream.path().to_str().unwrap())
        .expect("Failed to find repository");

    let ci_context = CiContext::with_repository(
        upstream_repo,
        CiEvent::Merge {
            merge_commit_sha: merge_sha,
            head_ref: "feature".to_string(),
            head_sha: feature_sha,
            base_ref: "main".to_string(),
            base_sha: base_commit.commit_sha.clone(),
            fork_clone_url: None, // Not a fork
        },
    );

    let result = ci_context.run().unwrap();

    // Should still be SkippedSimpleMerge for non-fork merge commits
    assert!(
        matches!(result, CiRunResult::SkippedSimpleMerge),
        "Expected SkippedSimpleMerge for non-fork merge commit, got {:?}",
        result
    );
}
