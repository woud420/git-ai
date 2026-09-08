use super::{ExpectedLineExt, GitAiRepository, direct_test_repo};

#[test]
fn test_ci_local_merge_squash_on_linear_main_does_not_note_base_commits() {
    let repo = direct_test_repo();
    repo.git_og(&["config", "user.name", "Test User"]).unwrap();
    repo.git_og(&["config", "user.email", "test@example.com"])
        .unwrap();

    // B0: initial commit on main (raw git -> no authorship note)
    std::fs::write(repo.path().join("base.txt"), "base content\n").unwrap();
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "B0 initial"]).unwrap();
    repo.git_og(&["branch", "-M", "main"]).unwrap();
    let b0_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // B1, B2, B3: teammate commits on main, NOT using the wrapper (no notes)
    for i in 1..=3 {
        std::fs::write(
            repo.path().join(format!("teammate{i}.txt")),
            format!("teammate change {i}\n"),
        )
        .unwrap();
        repo.git_og(&["add", "-A"]).unwrap();
        repo.git_og(&["commit", "-m", &format!("B{i} teammate change")])
            .unwrap();
    }
    let b2_sha = repo
        .git_og(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    let b3_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // feature branch off B0 with 3 AI commits (each gets a note via the wrapper)
    repo.git_og(&["checkout", "-b", "feature", &b0_sha])
        .unwrap();
    let mut feat = repo.filename("feature.txt");
    feat.set_contents(crate::lines!["// P1 ai line".ai()]);
    repo.stage_all_and_commit("P1").unwrap();
    feat.insert_at(1, crate::lines!["// P2 ai line".ai()]);
    repo.stage_all_and_commit("P2").unwrap();
    feat.insert_at(2, crate::lines!["// P3 ai line".ai()]);
    let head_sha = repo.stage_all_and_commit("P3").unwrap().commit_sha;

    // Squash merge: GitHub creates one new commit S on top of B3 (raw git)
    repo.git_og(&["checkout", "main"]).unwrap();
    std::fs::write(
        repo.path().join("feature.txt"),
        "// P1 ai line\n// P2 ai line\n// P3 ai line\n",
    )
    .unwrap();
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "Squash merge feature (#PR)"])
        .unwrap();
    let squash_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // Bare origin so `ci local merge` can push authorship
    let origin_dir = tempfile::tempdir().unwrap();
    let origin_path = origin_dir.path().join("origin.git");
    repo.git_og(&[
        "clone",
        "--bare",
        repo.path().to_str().unwrap(),
        origin_path.to_str().unwrap(),
    ])
    .unwrap();
    repo.git_og(&["remote", "add", "origin", origin_path.to_str().unwrap()])
        .unwrap();

    // Run the real CLI exactly as CI would after a squash merge
    let output = repo
        .git_ai(&[
            "ci",
            "local",
            "merge",
            "--merge-commit-sha",
            squash_sha.as_str(),
            "--head-ref",
            "feature",
            "--head-sha",
            head_sha.as_str(),
            "--base-ref",
            "main",
            "--base-sha",
            b3_sha.as_str(),
            "--skip-fetch-notes",
            "--skip-fetch-base",
        ])
        .expect("ci local merge should succeed");

    assert!(
        output.contains("authorship rewritten successfully"),
        "expected authorship rewritten, got: {output}"
    );

    // Only the squash commit S carries a note; the base commits are untouched.
    assert!(
        repo.read_authorship_note(&squash_sha).is_some(),
        "squash commit S ({squash_sha}) should receive the rewritten authorship note"
    );
    assert!(
        repo.read_authorship_note(&b2_sha).is_none(),
        "#1473 regression: unrelated base commit B2 ({b2_sha}) must not receive a note"
    );
    assert!(
        repo.read_authorship_note(&b3_sha).is_none(),
        "#1473 regression: unrelated base commit B3 ({b3_sha}) must not receive a note"
    );
}

/// Regression test for #1473: a squash merge of a multi-commit PR onto a *linear*
/// main branch must not be misclassified as a rebase merge and pollute unrelated
/// base commits. Drives `CiContext::run_with_options` directly (still supported).
#[test]
fn test_ci_squash_merge_not_misclassified_as_rebase_on_linear_main() {
    use git_ai::operations::ci::ci_context::{CiContext, CiEvent, CiRunOptions};

    let repo = direct_test_repo();
    repo.git_og(&["config", "user.name", "Test User"]).unwrap();
    repo.git_og(&["config", "user.email", "test@example.com"])
        .unwrap();

    // B0: initial commit on main (raw git -> no authorship note).
    std::fs::write(repo.path().join("base.txt"), "base content\n").unwrap();
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "B0 initial"]).unwrap();
    repo.git_og(&["branch", "-M", "main"]).unwrap();
    let b0_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // B1, B2, B3: teammate commits on main, no notes.
    for i in 1..=3 {
        std::fs::write(
            repo.path().join(format!("teammate{i}.txt")),
            format!("teammate change {i}\n"),
        )
        .unwrap();
        repo.git_og(&["add", "-A"]).unwrap();
        repo.git_og(&["commit", "-m", &format!("B{i} teammate change")])
            .unwrap();
    }
    let b2_sha = repo
        .git_og(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    let b3_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    // feature branch off B0 with 3 AI commits (each gets a note via the wrapper).
    repo.git_og(&["checkout", "-b", "feature", &b0_sha])
        .unwrap();
    let mut feat = repo.filename("feature.txt");
    feat.set_contents(crate::lines!["// P1 ai line".ai()]);
    repo.stage_all_and_commit("P1").unwrap();
    feat.insert_at(1, crate::lines!["// P2 ai line".ai()]);
    repo.stage_all_and_commit("P2").unwrap();
    feat.insert_at(2, crate::lines!["// P3 ai line".ai()]);
    let head_sha = repo.stage_all_and_commit("P3").unwrap().commit_sha;

    // Squash merge: one new commit S on top of B3 (raw git).
    repo.git_og(&["checkout", "main"]).unwrap();
    std::fs::write(
        repo.path().join("feature.txt"),
        "// P1 ai line\n// P2 ai line\n// P3 ai line\n",
    )
    .unwrap();
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "Squash merge feature (#PR)"])
        .unwrap();
    let squash_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let git_ai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");
    let event = CiEvent::Merge {
        merge_commit_sha: squash_sha.clone(),
        head_ref: "feature".to_string(),
        head_sha: head_sha.clone(),
        base_ref: "main".to_string(),
        base_sha: b3_sha.clone(),
        fork_clone_url: None,
    };
    let ctx = CiContext::with_repository(git_ai_repo, event);
    ctx.run_with_options(CiRunOptions {
        skip_fetch_notes: true,
        skip_fetch_base: true,
        skip_fetch_fork_notes: true,
        skip_fetch_sync_refs: false,
        skip_push: true,
    })
    .expect("CI merge rewrite should succeed");

    // S should be attributed; unrelated base commits B2/B3 must not be polluted.
    assert!(
        repo.read_authorship_note(&squash_sha).is_some(),
        "squash commit S ({squash_sha}) should receive the rewritten authorship note"
    );
    assert!(
        repo.read_authorship_note(&b2_sha).is_none(),
        "#1473 regression: unrelated base commit B2 ({b2_sha}) must not receive a note"
    );
    assert!(
        repo.read_authorship_note(&b3_sha).is_none(),
        "#1473 regression: unrelated base commit B3 ({b3_sha}) must not receive a note"
    );
}

crate::reuse_tests_in_worktree!(
    test_ci_local_merge_squash_on_linear_main_does_not_note_base_commits,
    test_ci_squash_merge_not_misclassified_as_rebase_on_linear_main,
);
