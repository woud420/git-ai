use super::{TestRepo, fs};

#[test]
fn test_cross_repo_checkpoint_creates_working_log_in_target_repo() {
    let repo1 = TestRepo::new();
    let repo2 = TestRepo::new();

    let mut file = repo2.filename("test.txt");
    file.set_contents(crate::lines!["Line 1", "Line 2", "Line 3"]);
    repo2.stage_all_and_commit("Initial commit").unwrap();

    fs::write(
        repo2.path().join("test.txt"),
        "Line 1\nLine 2\nLine 3\nAI Line 1\nAI Line 2\n",
    )
    .unwrap();

    let repo2_file_abs = repo2.canonical_path().join("test.txt");
    let abs_path_str = repo2_file_abs.to_str().unwrap();

    repo2
        .git_ai_from_working_dir(
            &repo1.canonical_path(),
            &["checkpoint", "mock_ai", abs_path_str],
        )
        .unwrap();

    let working_log = repo2.current_working_logs();
    let ai_files = working_log.all_ai_touched_files().unwrap_or_default();
    assert!(
        !ai_files.is_empty(),
        "Cross-repo checkpoint should create working log entries in the target repo (repo2), but found none. \
         This means the checkpoint from repo1's working directory failed to write to repo2's working log."
    );
}

#[test]
fn test_cross_repo_checkpoint_ai_attribution_on_commit() {
    let repo1 = TestRepo::new();
    let repo2 = TestRepo::new();

    let mut file = repo2.filename("test.txt");
    file.set_contents(crate::lines!["Line 1", "Line 2", "Line 3"]);
    repo2.stage_all_and_commit("Initial commit").unwrap();

    fs::write(
        repo2.path().join("test.txt"),
        "Line 1\nLine 2\nLine 3\nAI Line 1\nAI Line 2\n",
    )
    .unwrap();

    let repo2_file_abs = repo2.canonical_path().join("test.txt");
    let abs_path_str = repo2_file_abs.to_str().unwrap();

    repo2
        .git_ai_from_working_dir(
            &repo1.canonical_path(),
            &["checkpoint", "mock_ai", abs_path_str],
        )
        .unwrap();

    let commit = repo2.stage_all_and_commit("AI edits from repo1").unwrap();

    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "Cross-repo AI edits should be attributed to AI, not human. \
         The checkpoint was run from repo1's working directory for a file in repo2, \
         but the commit in repo2 shows no AI attestations."
    );
}

#[test]
fn test_cross_repo_checkpoint_preserves_local_repo_checkpoint() {
    let repo1 = TestRepo::new();
    let repo2 = TestRepo::new();

    let mut file1 = repo1.filename("local.txt");
    file1.set_contents(crate::lines!["Local 1", "Local 2"]);
    repo1
        .stage_all_and_commit("Initial commit in repo1")
        .unwrap();

    let mut file2 = repo2.filename("remote.txt");
    file2.set_contents(crate::lines!["Remote 1", "Remote 2"]);
    repo2
        .stage_all_and_commit("Initial commit in repo2")
        .unwrap();

    fs::write(
        repo1.path().join("local.txt"),
        "Local 1\nLocal 2\nAI local line\n",
    )
    .unwrap();
    fs::write(
        repo2.path().join("remote.txt"),
        "Remote 1\nRemote 2\nAI remote line\n",
    )
    .unwrap();

    let repo1_file_abs = repo1.canonical_path().join("local.txt");
    let repo2_file_abs = repo2.canonical_path().join("remote.txt");

    repo1
        .git_ai_from_working_dir(
            &repo1.canonical_path(),
            &[
                "checkpoint",
                "mock_ai",
                repo1_file_abs.to_str().unwrap(),
                repo2_file_abs.to_str().unwrap(),
            ],
        )
        .unwrap();

    let repo1_commit = repo1.stage_all_and_commit("AI edits in repo1").unwrap();
    assert!(
        !repo1_commit.authorship_log.attestations.is_empty(),
        "Local repo (repo1) should still have AI attestations when cross-repo files are also checkpointed"
    );

    let repo2_commit = repo2.stage_all_and_commit("AI edits in repo2").unwrap();
    assert!(
        !repo2_commit.authorship_log.attestations.is_empty(),
        "Cross-repo (repo2) should also have AI attestations"
    );
}
