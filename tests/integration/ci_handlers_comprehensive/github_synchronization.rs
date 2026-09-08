use super::{TestRepo, path_str, run_real_git};
use std::io::Write;

// ==============================================================================
// CI Result Types Tests
// ==============================================================================

#[test]
fn test_ci_result_types_coverage() {
    // Test that we understand all CiRunResult variants
    use git_ai::model::authorship_log_serialization::AuthorshipLog;
    use git_ai::operations::ci::ci_context::CiRunResult;

    // Test variant construction
    let result1 = CiRunResult::AuthorshipRewritten {
        authorship_log: AuthorshipLog::default(),
    };
    let result2 = CiRunResult::AlreadyExists {
        authorship_log: AuthorshipLog::default(),
    };
    let result3 = CiRunResult::SkippedSimpleMerge;
    let result4 = CiRunResult::SkippedFastForward;
    let result5 = CiRunResult::NoAuthorshipAvailable;
    let result6 = CiRunResult::SyncAuthorshipRewritten { commit_count: 2 };
    let result7 = CiRunResult::SkippedExistingSyncNotes;

    // Verify variants can be constructed
    match result1 {
        CiRunResult::AuthorshipRewritten { .. } => {}
        _ => panic!("Expected AuthorshipRewritten"),
    }

    match result2 {
        CiRunResult::AlreadyExists { .. } => {}
        _ => panic!("Expected AlreadyExists"),
    }

    match result3 {
        CiRunResult::SkippedSimpleMerge => {}
        _ => panic!("Expected SkippedSimpleMerge"),
    }

    match result4 {
        CiRunResult::SkippedFastForward => {}
        _ => panic!("Expected SkippedFastForward"),
    }

    match result5 {
        CiRunResult::NoAuthorshipAvailable => {}
        _ => panic!("Expected NoAuthorshipAvailable"),
    }

    match result6 {
        CiRunResult::SyncAuthorshipRewritten { commit_count } => assert_eq!(commit_count, 2),
        _ => panic!("Expected SyncAuthorshipRewritten"),
    }

    match result7 {
        CiRunResult::SkippedExistingSyncNotes => {}
        _ => panic!("Expected SkippedExistingSyncNotes"),
    }
}

#[test]
fn test_ci_github_run_noops_when_synchronize_has_no_previous_head() {
    let repo = TestRepo::new();
    let mut event_file = tempfile::NamedTempFile::new().expect("event file");
    write!(
        event_file,
        r#"{{
          "action": "synchronize",
          "before": "0000000000000000000000000000000000000000",
          "after": "2222222222222222222222222222222222222222",
          "pull_request": {{
            "number": 42,
            "merged": false,
            "merge_commit_sha": null,
            "base": {{
              "ref": "main",
              "sha": "1111111111111111111111111111111111111111",
              "repo": {{ "clone_url": "https://github.com/acme/repo.git" }}
            }},
            "head": {{
              "ref": "feature",
              "sha": "2222222222222222222222222222222222222222",
              "repo": {{ "clone_url": "https://github.com/acme/repo.git" }}
            }}
          }}
        }}"#
    )
    .expect("write event");

    let output = repo
        .git_ai_with_env(
            &["ci", "github", "run", "--no-cleanup"],
            &[
                ("GITHUB_EVENT_NAME", "pull_request"),
                (
                    "GITHUB_EVENT_PATH",
                    event_file.path().to_str().expect("event path"),
                ),
            ],
        )
        .expect("github ci run should no-op successfully");

    assert!(
        output.contains("No GitHub CI context found; nothing to do"),
        "Expected no-op output, got: {}",
        output
    );
}

#[test]
fn test_ci_github_run_fetches_missing_previous_head_after_force_push() {
    let ci_repo = TestRepo::new();
    let tmp = tempfile::tempdir().expect("tempdir");
    let remote_path = tmp.path().join("origin.git");
    let work_path = tmp.path().join("work");
    let remote = path_str(&remote_path);
    let work = path_str(&work_path);

    run_real_git(&["init", "--bare", "--initial-branch=main", remote]);
    run_real_git(&[
        "-C",
        remote,
        "config",
        "uploadpack.allowAnySHA1InWant",
        "true",
    ]);
    run_real_git(&["init", "--initial-branch=main", work]);
    run_real_git(&["-C", work, "config", "user.name", "Test User"]);
    run_real_git(&["-C", work, "config", "user.email", "test@example.com"]);

    std::fs::write(work_path.join("file.txt"), "base\n").expect("write base");
    run_real_git(&["-C", work, "add", "file.txt"]);
    run_real_git(&["-C", work, "commit", "-m", "base"]);
    let base_sha = run_real_git(&["-C", work, "rev-parse", "HEAD"]);
    run_real_git(&["-C", work, "remote", "add", "origin", remote]);
    run_real_git(&["-C", work, "push", "origin", "main"]);

    run_real_git(&["-C", work, "checkout", "-b", "feature"]);
    std::fs::write(work_path.join("file.txt"), "old PR head\n").expect("write old head");
    run_real_git(&["-C", work, "commit", "-am", "old pr head"]);
    let previous_head_sha = run_real_git(&["-C", work, "rev-parse", "HEAD"]);
    run_real_git(&["-C", work, "push", "origin", "HEAD:refs/pull/42/head"]);

    run_real_git(&["-C", work, "checkout", "-B", "feature", "main"]);
    std::fs::write(work_path.join("file.txt"), "new PR head\n").expect("write new head");
    run_real_git(&["-C", work, "commit", "-am", "new pr head"]);
    let current_head_sha = run_real_git(&["-C", work, "rev-parse", "HEAD"]);
    run_real_git(&[
        "-C",
        work,
        "push",
        "--force",
        "origin",
        "HEAD:refs/pull/42/head",
    ]);

    let remote_url =
        url::Url::from_directory_path(&remote_path).expect("remote path should be file URL");
    let mut event_file = tempfile::NamedTempFile::new().expect("event file");
    let event = serde_json::json!({
        "action": "synchronize",
        "before": previous_head_sha,
        "after": current_head_sha,
        "pull_request": {
            "number": 42,
            "merged": false,
            "merge_commit_sha": null,
            "base": {
                "ref": "main",
                "sha": base_sha,
                "repo": { "clone_url": remote_url.as_str() }
            },
            "head": {
                "ref": "feature",
                "sha": current_head_sha,
                "repo": { "clone_url": remote_url.as_str() }
            }
        }
    });
    serde_json::to_writer(&mut event_file, &event).expect("write event");
    event_file.flush().expect("flush event");

    let output = ci_repo
        .git_ai_with_env(
            &["ci", "github", "run", "--no-cleanup"],
            &[
                ("GITHUB_EVENT_NAME", "pull_request"),
                (
                    "GITHUB_EVENT_PATH",
                    event_file.path().to_str().expect("event path"),
                ),
            ],
        )
        .expect("github ci run should fetch the missing previous head");

    assert!(
        output.contains("GitHub CI: skipped non-rebase PR sync"),
        "expected successful non-rebase sync skip, got:\n{}",
        output
    );

    let clone_path = ci_repo.path().join("git-ai-ci-clone");
    let clone = path_str(&clone_path);
    let previous_commit = format!("{}^{{commit}}", previous_head_sha);
    let resolved_previous_head =
        run_real_git(&["-C", clone, "rev-parse", "--verify", &previous_commit]);
    assert_eq!(resolved_previous_head, previous_head_sha);
}

crate::reuse_tests_in_worktree!(test_ci_result_types_coverage,);
