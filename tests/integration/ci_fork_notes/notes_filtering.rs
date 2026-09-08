use super::{
    CiContext, CiEvent, CiRunResult, ExpectedLineExt, GitAiRepository, TestRepo, add_self_origin,
    fs, read_authorship_v3, read_note, write_note,
};

/// Test that CI handles fork PRs with no notes gracefully.
///
/// If a fork contributor doesn't use git-ai, there are no notes to fetch.
/// The CI should handle this gracefully without errors.
#[test]
fn test_ci_fork_no_notes() {
    // Setup upstream
    let upstream = TestRepo::new();
    let mut file = upstream.filename("feature.js");

    file.set_contents(lines!["// Original code"]);
    let base_commit = upstream.stage_all_and_commit("Initial commit").unwrap();
    upstream.git(&["branch", "-M", "main"]).unwrap();
    add_self_origin(&upstream);

    // Create fork WITHOUT git-ai (using git_og for all operations)
    let fork = TestRepo::new();
    let upstream_path = upstream.path().to_str().unwrap().to_string();
    fork.git_og(&["remote", "add", "upstream", &upstream_path])
        .unwrap();
    fork.git_og(&["fetch", "upstream"]).unwrap();
    fork.git_og(&["checkout", "-b", "main", "upstream/main"])
        .unwrap();

    // Fork contributor adds code without git-ai
    let mut fork_file = fork.filename("feature.js");
    fork_file.set_contents(lines!["// Original code", "// Added in fork"]);
    fork.git_og(&["add", "-A"]).unwrap();
    fork.git_og(&["commit", "-m", "Add feature in fork"])
        .unwrap();
    let fork_head_sha = fork
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Fetch fork commits into upstream
    upstream
        .git_og(&["remote", "add", "fork", fork.path().to_str().unwrap()])
        .unwrap();
    upstream
        .git_og(&["fetch", "fork", "main:refs/fork/main"])
        .unwrap();

    // Simulate squash merge in upstream (using raw git to avoid auto-notes)
    file.set_contents(lines!["// Original code", "// Added in fork"]);
    upstream.git_og(&["add", "-A"]).unwrap();
    upstream
        .git_og(&["commit", "-m", "Merge fork PR via squash"])
        .unwrap();
    let merge_sha = upstream
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Run CI with fork URL
    let upstream_repo = GitAiRepository::find_repository_in_path(upstream.path().to_str().unwrap())
        .expect("Failed to find upstream repository");

    let fork_url = fork.path().to_str().unwrap().to_string();

    let ci_context = CiContext::with_repository(
        upstream_repo,
        CiEvent::Merge {
            merge_commit_sha: merge_sha.clone(),
            head_ref: "main".to_string(),
            head_sha: fork_head_sha,
            base_ref: "main".to_string(),
            base_sha: base_commit.commit_sha.clone(),
            fork_clone_url: Some(fork_url),
        },
    );

    // Should complete without errors, even though fork has no notes
    let result = ci_context.run().unwrap();
    assert!(
        matches!(result, CiRunResult::NoAuthorshipAvailable),
        "Expected NoAuthorshipAvailable for fork with no git-ai notes, got {:?}",
        result
    );
}

/// A fork notes ref is untrusted input. CI must import only notes attached to
/// commits that are actually in the PR, not every note present in the fork.
#[test]
fn test_ci_fork_notes_ignores_notes_outside_pr_commit_range() {
    let upstream = TestRepo::new();
    let upstream_file = upstream.path().join("feature.js");

    fs::write(&upstream_file, "// Original code\n").unwrap();
    upstream.git_og(&["add", "-A"]).unwrap();
    upstream
        .git_og(&["commit", "-m", "Initial commit"])
        .unwrap();
    let base_sha = upstream
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    upstream.git(&["branch", "-M", "main"]).unwrap();
    add_self_origin(&upstream);

    let fork = TestRepo::new();
    let upstream_path = upstream.path().to_str().unwrap().to_string();
    fork.git_og(&["remote", "add", "upstream", &upstream_path])
        .unwrap();
    fork.git_og(&["fetch", "upstream"]).unwrap();
    fork.git_og(&["checkout", "-b", "main", "upstream/main"])
        .unwrap();

    let fork_repo = GitAiRepository::find_repository_in_path(fork.path().to_str().unwrap())
        .expect("Failed to find fork repository");
    write_note(&fork_repo, &base_sha, "malicious note for upstream base")
        .expect("add malicious fork note");

    let mut fork_file = fork.filename("feature.js");
    fork_file.set_contents(lines![
        "// Original code",
        "// AI feature from fork".ai(),
        "function forkFeature() {}".ai()
    ]);
    let fork_commit = fork.stage_all_and_commit("Add AI feature in fork").unwrap();
    let fork_head_sha = fork_commit.commit_sha.clone();

    upstream
        .git_og(&["remote", "add", "fork", fork.path().to_str().unwrap()])
        .unwrap();
    upstream
        .git_og(&["fetch", "fork", "main:refs/fork/main"])
        .unwrap();

    fs::write(
        &upstream_file,
        "// Original code\n// AI feature from fork\nfunction forkFeature() {}\n",
    )
    .unwrap();
    upstream.git_og(&["add", "-A"]).unwrap();
    upstream
        .git_og(&["commit", "-m", "Merge fork PR via squash"])
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
            base_sha: base_sha.clone(),
            fork_clone_url: Some(fork.path().to_str().unwrap().to_string()),
        },
    );

    let result = ci_context.run().unwrap();
    assert!(
        matches!(result, CiRunResult::AuthorshipRewritten { .. }),
        "Expected AuthorshipRewritten for fork squash merge, got {:?}",
        result
    );

    let upstream_repo_after =
        GitAiRepository::find_repository_in_path(upstream.path().to_str().unwrap())
            .expect("Failed to find upstream repository");
    assert!(
        read_note(&upstream_repo_after, &base_sha).is_none(),
        "malicious fork note for base commit must not be imported"
    );
    assert!(
        read_authorship_v3(&upstream_repo_after, &merge_sha).is_ok(),
        "squash commit should still receive rewritten authorship from PR commit notes"
    );
}

/// Test merge commit from fork with no notes anywhere.
///
/// If neither origin nor fork has refs/notes/ai, CI should not attempt to push
/// notes for a fork merge commit and should skip gracefully.
#[test]
fn test_ci_fork_merge_commit_no_notes_skips_without_push_error() {
    let upstream = TestRepo::new();
    let mut file = upstream.filename("feature.js");

    file.set_contents(lines!["// Original code"]);
    upstream.git_og(&["add", "-A"]).unwrap();
    upstream
        .git_og(&["commit", "-m", "Initial commit"])
        .unwrap();
    let base_sha = upstream
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    upstream.git(&["branch", "-M", "main"]).unwrap();
    add_self_origin(&upstream);

    // Create fork WITHOUT git-ai notes
    let fork = TestRepo::new();
    let upstream_path = upstream.path().to_str().unwrap().to_string();
    fork.git_og(&["remote", "add", "upstream", &upstream_path])
        .unwrap();
    fork.git_og(&["fetch", "upstream"]).unwrap();
    fork.git_og(&["checkout", "-b", "main", "upstream/main"])
        .unwrap();

    let mut fork_file = fork.filename("feature.js");
    fork_file.set_contents(lines!["// Original code", "// Added in fork via merge"]);
    fork.git_og(&["add", "-A"]).unwrap();
    fork.git_og(&["commit", "-m", "Add feature in fork"])
        .unwrap();
    let fork_head_sha = fork
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Fetch fork branch and merge with merge commit
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
            "Merge fork PR with no notes",
        ])
        .unwrap();
    let merge_sha = upstream
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let upstream_repo = GitAiRepository::find_repository_in_path(upstream.path().to_str().unwrap())
        .expect("Failed to find upstream repository");

    let fork_url = fork.path().to_str().unwrap().to_string();

    let ci_context = CiContext::with_repository(
        upstream_repo,
        CiEvent::Merge {
            merge_commit_sha: merge_sha,
            head_ref: "main".to_string(),
            head_sha: fork_head_sha,
            base_ref: "main".to_string(),
            base_sha,
            fork_clone_url: Some(fork_url),
        },
    );

    let result = ci_context.run().unwrap();
    assert!(
        matches!(result, CiRunResult::SkippedSimpleMerge),
        "Expected SkippedSimpleMerge for fork merge commit with no notes, got {:?}",
        result
    );
}
