use super::{ExpectedLineExt, GitAiRepository, direct_test_repo, write_note};

#[test]
fn test_ci_local_sync_skips_when_current_rebased_commit_already_has_note() {
    let repo = direct_test_repo();

    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git_og(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["ai content".ai()]);
    let previous_head_sha = repo.stage_all_and_commit("Add feature").unwrap().commit_sha;

    repo.git_og(&["checkout", "main"]).unwrap();
    let mut main_file = repo.filename("main_only.txt");
    main_file.set_contents(crate::lines!["main-only content"]);
    repo.git_og(&["add", "main_only.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "Advance main"]).unwrap();
    let base_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    repo.git_og(&["checkout", "feature"]).unwrap();
    repo.git_og(&["rebase", "main"]).unwrap();
    let current_head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let gitai_repo =
        GitAiRepository::find_repository_in_path(repo.path().to_str().expect("repo path"))
            .expect("git-ai repo");
    let existing_note = "client-side-note-that-ci-must-not-overwrite";
    write_note(&gitai_repo, &current_head_sha, existing_note).expect("add existing current note");

    let output = repo
        .git_ai(&[
            "ci",
            "local",
            "sync",
            "--previous-head-sha",
            previous_head_sha.as_str(),
            "--base-ref",
            "main",
            "--base-sha",
            base_sha.as_str(),
            "--head-sha",
            current_head_sha.as_str(),
            "--skip-fetch-notes",
            "--skip-push",
        ])
        .expect("ci local sync should succeed");

    assert!(
        output.contains("Local CI (sync): skipped PR sync with existing authorship"),
        "Expected existing-note skip, got: {}",
        output
    );
    let current_note = repo
        .read_authorship_note(&current_head_sha)
        .map(|note| note.trim().to_string());
    assert_eq!(
        current_note.as_deref(),
        Some(existing_note),
        "CI sync must not overwrite a current commit note that already exists"
    );
}

#[test]
fn test_ci_local_sync_skips_non_rebase_force_push() {
    let repo = direct_test_repo();

    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git_og(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["old ai content".ai()]);
    let previous_head_sha = repo
        .stage_all_and_commit("Add old AI content")
        .unwrap()
        .commit_sha;
    assert!(
        repo.read_authorship_note(&previous_head_sha).is_some(),
        "old PR head should have an authorship note"
    );

    repo.git_og(&["reset", "--hard", "main"]).unwrap();
    feature_file.set_contents(crate::lines!["different force-pushed content"]);
    repo.git_og(&["add", "feature.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "Force-pushed replacement"])
        .unwrap();
    let current_head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let output = repo
        .git_ai(&[
            "ci",
            "local",
            "sync",
            "--previous-head-sha",
            previous_head_sha.as_str(),
            "--base-ref",
            "main",
            "--head-sha",
            current_head_sha.as_str(),
            "--skip-fetch-notes",
            "--skip-fetch-sync-refs",
            "--skip-push",
        ])
        .expect("ci local sync should succeed for non-rebase force push");

    assert!(
        output.contains("Local CI (sync): skipped non-rebase PR sync"),
        "Expected non-rebase sync skip, got: {}",
        output
    );
    assert!(
        repo.read_authorship_note(&current_head_sha).is_none(),
        "non-rebase sync must not transfer old authorship to unrelated replacement commit"
    );
}

#[test]
fn test_ci_local_open_pr_rebase_single_commit() {
    let repo = direct_test_repo();

    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git_og(&["checkout", "-b", "feature"]).unwrap();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.set_contents(crate::lines!["ai content".ai()]);
    let previous_head_sha = repo.stage_all_and_commit("Add feature").unwrap().commit_sha;

    repo.git_og(&["checkout", "main"]).unwrap();
    let mut main_file = repo.filename("main_only.txt");
    main_file.set_contents(crate::lines!["main-only content"]);
    repo.git_og(&["add", "main_only.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "Advance main"]).unwrap();
    let base_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    repo.git_og(&["checkout", "feature"]).unwrap();
    repo.git_og(&["rebase", "main"]).unwrap();
    let current_head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    assert_ne!(current_head_sha, previous_head_sha);
    assert!(
        repo.read_authorship_note(&current_head_sha).is_none(),
        "bypassed rebase should not pre-create note for the rebased commit"
    );

    let output = repo
        .git_ai(&[
            "ci",
            "local",
            "sync",
            "--previous-head-sha",
            previous_head_sha.as_str(),
            "--base-ref",
            "main",
            "--base-sha",
            base_sha.as_str(),
            "--head-sha",
            current_head_sha.as_str(),
            "--skip-fetch-notes",
            "--skip-push",
        ])
        .expect("ci local sync should succeed");

    assert!(
        output.contains("Local CI (sync): authorship rewritten successfully"),
        "Expected authorship rewritten, got: {}",
        output
    );

    let files: Vec<String> = repo
        .require_authorship_log(&current_head_sha)
        .attestations
        .iter()
        .map(|a| a.file_path.clone())
        .collect();
    assert!(
        files.iter().any(|f| f.contains("feature.txt")),
        "rebased single PR commit should reference feature.txt, got: {:?}",
        files
    );
}

#[test]
fn test_ci_local_open_pr_rebase_two_commits() {
    use git_ai::model::authorship_log_serialization::AuthorshipLog;

    let repo = direct_test_repo();

    // --- Initial commit on main ---
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base content"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    repo.git(&["branch", "-M", "main"]).unwrap();

    // --- Feature branch: two AI commits touching distinct files ---
    repo.git_og(&["checkout", "-b", "feature"]).unwrap();

    let mut file_a = repo.filename("file_a.txt");
    file_a.set_contents(crate::lines!["ai content in file_a".ai()]);
    let feature_sha1 = repo.stage_all_and_commit("Add file_a").unwrap().commit_sha;

    let mut file_b = repo.filename("file_b.txt");
    file_b.set_contents(crate::lines!["ai content in file_b".ai()]);
    let feature_sha2 = repo.stage_all_and_commit("Add file_b").unwrap().commit_sha;

    let previous_head_sha = feature_sha2.clone();

    // --- Advance main so the open-PR rebase produces new SHAs ---
    repo.git_og(&["checkout", "main"]).unwrap();
    let mut main_file = repo.filename("main_only.txt");
    main_file.set_contents(crate::lines!["main-only content"]);
    repo.git_og(&["add", "main_only.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "Advance main"]).unwrap();
    let base_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // --- Rebase the open feature branch onto main, bypassing local hooks ---
    repo.git_og(&["checkout", "feature"]).unwrap();
    repo.git_og(&["rebase", "main"]).unwrap();

    let new_sha2 = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let new_sha1 = repo
        .git_og(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();

    assert_ne!(
        new_sha1, feature_sha1,
        "open-PR rebase must produce a new SHA for commit 1"
    );
    assert_ne!(
        new_sha2, feature_sha2,
        "open-PR rebase must produce a new SHA for commit 2"
    );
    assert!(
        repo.read_authorship_note(&new_sha1).is_none(),
        "bypassed rebase should not pre-create note for commit 1"
    );
    assert!(
        repo.read_authorship_note(&new_sha2).is_none(),
        "bypassed rebase should not pre-create note for commit 2"
    );

    // --- Run the new open-PR sync command ---
    let spawn_log = repo.test_home_path().join("ci-sync-spawns.log");
    let output = repo
        .git_ai_with_env(
            &[
                "ci",
                "local",
                "sync",
                "--previous-head-sha",
                previous_head_sha.as_str(),
                "--base-ref",
                "main",
                "--base-sha",
                base_sha.as_str(),
                "--head-sha",
                new_sha2.as_str(),
                "--skip-fetch-notes",
                "--skip-push",
            ],
            &[(
                "GIT_AI_SPAWN_LOG",
                spawn_log.to_str().expect("spawn log path should be UTF-8"),
            )],
        )
        .expect("ci local sync should succeed");

    let spawns = std::fs::read_to_string(&spawn_log).expect("read CI sync spawn log");
    assert_eq!(
        spawns.lines().filter(|line| *line == "log").count(),
        1,
        "CI range comparison should read all commit patches with one git log process; spawns:\n{spawns}"
    );
    assert_eq!(
        spawns.lines().filter(|line| *line == "patch-id").count(),
        1,
        "CI range comparison should batch all commits into one patch-id process; spawns:\n{spawns}"
    );

    assert!(
        output.contains("Local CI (sync): authorship rewritten successfully"),
        "Expected authorship rewritten, got: {}",
        output
    );

    // --- Verify each rebased open-PR commit carries notes for its own file ---
    let note1 = repo
        .read_authorship_note(&new_sha1)
        .expect("rebased PR commit 1 should have an authorship note");
    let note2 = repo
        .read_authorship_note(&new_sha2)
        .expect("rebased PR commit 2 should have an authorship note");

    let files1: Vec<String> = AuthorshipLog::deserialize_from_string(&note1)
        .unwrap()
        .attestations
        .iter()
        .map(|a| a.file_path.clone())
        .collect();
    let files2: Vec<String> = AuthorshipLog::deserialize_from_string(&note2)
        .unwrap()
        .attestations
        .iter()
        .map(|a| a.file_path.clone())
        .collect();

    assert!(
        files1.iter().any(|f| f.contains("file_a")),
        "rebased PR commit 1 should reference file_a.txt, got: {:?}",
        files1
    );
    assert!(
        !files1.iter().any(|f| f.contains("file_b")),
        "rebased PR commit 1 should not reference file_b.txt, got: {:?}",
        files1
    );
    assert!(
        files2.iter().any(|f| f.contains("file_b")),
        "rebased PR commit 2 should reference file_b.txt, got: {:?}",
        files2
    );
    assert!(
        !files2.iter().any(|f| f.contains("file_a")),
        "rebased PR commit 2 should not reference file_a.txt, got: {:?}",
        files2
    );
}

crate::reuse_tests_in_worktree!(
    test_ci_local_sync_skips_when_current_rebased_commit_already_has_note,
    test_ci_local_sync_skips_non_rebase_force_push,
    test_ci_local_open_pr_rebase_single_commit,
    test_ci_local_open_pr_rebase_two_commits,
);
