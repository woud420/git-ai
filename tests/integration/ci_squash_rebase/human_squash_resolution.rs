use super::{
    ExpectedLineExt, assert_ci_rewrite_succeeded, direct_test_repo, run_ci_local_merge,
    squash_feature_with_raw_git,
};

/// Squash merge where the feature branch mixes human and AI lines. The CI
/// rewrite must split attribution by author across the single squashed commit.
/// (Originally exercised the removed `rewrite_authorship_after_squash_or_rebase`
/// engine directly; now driven through the real `git-ai ci local merge` CLI.)
#[test]
fn test_ci_squash_merge_mixed_content() {
    let repo = direct_test_repo();
    let mut file = repo.filename("mixed.js");

    // Initial commit on main.
    file.set_contents(crate::lines!["// Base code", "const base = 1;"]);
    let base_sha = repo
        .stage_all_and_commit("Initial commit")
        .unwrap()
        .commit_sha;
    repo.git(&["branch", "-M", "main"]).unwrap();

    // Feature branch: known-human comment, AI code, known-human comment.
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.set_contents(crate::lines![
        "// Base code".human(),
        "const base = 1;".human(),
        "// Human comment".human(),
        "// AI generated function".ai(),
        "function aiHelper() {".ai(),
        "  return true;".ai(),
        "}".ai(),
        "// Another human comment".human()
    ]);
    let head_sha = repo
        .stage_all_and_commit("Add mixed content")
        .unwrap()
        .commit_sha;

    // CI squash merge: a single new commit on main with the squashed content.
    let merge_sha = squash_feature_with_raw_git(&repo, "Merge feature via squash");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    // The squashed commit splits attribution by author: known-human lines stay
    // human and the AI block stays AI.
    file.assert_lines_and_blame(crate::lines![
        "// Base code".human(),
        "const base = 1;".human(),
        "// Human comment".human(),
        "// AI generated function".ai(),
        "function aiHelper() {".ai(),
        "  return true;".ai(),
        "}".ai(),
        "// Another human comment".human()
    ]);
}

/// Squash merge where extra lines are added during the merge (conflict
/// resolution / manual tweaks): AI lines stay AI, manually-added lines are
/// untracked human. Originally exercised the removed engine directly.
#[test]
fn test_ci_squash_merge_with_manual_changes() {
    let repo = direct_test_repo();
    let mut file = repo.filename("config.js");

    file.set_contents(crate::lines!["const config = {", "  version: 1", "};"]);
    let base_sha = repo
        .stage_all_and_commit("Initial commit")
        .unwrap()
        .commit_sha;
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.set_contents(crate::lines![
        "const config = {",
        "  version: 1,",
        "  // AI added feature flag".ai(),
        "  enableAI: true".ai(),
        "};"
    ]);
    let head_sha = repo
        .stage_all_and_commit("Add AI config")
        .unwrap()
        .commit_sha;

    // Squash onto main, then add manual lines before the CI rewrite runs.
    repo.git_og(&["checkout", "main"]).unwrap();
    repo.git_og(&["merge", "--squash", "feature"]).unwrap();
    file.set_contents(crate::lines![
        "const config = {",
        "  version: 1,",
        "  // AI added feature flag",
        "  enableAI: true,",
        "  // Manual addition during merge",
        "  production: false",
        "};"
    ]);
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "Merge feature via squash with tweaks"])
        .unwrap();
    let merge_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    // New-logic behavior (content-based reconciliation): only the AI line whose
    // committed content is byte-identical to the AI checkpoint keeps AI
    // attribution. `enableAI: true,` gained a trailing comma during the squash,
    // so its committed content differs from the AI-authored `enableAI: true` and
    // it is attributed to the committer (human) -- along with the manually-added
    // lines. (The removed engine attributed `enableAI: true,` to AI; this is the
    // intended tightening under the rewrite.)
    file.assert_lines_and_blame(crate::lines![
        "const config = {".human(),
        "  version: 1,".human(),
        "  // AI added feature flag".ai(),
        "  enableAI: true,".human(),
        "  // Manual addition during merge".human(),
        "  production: false".human(),
        "};".human()
    ]);
}

/// Legacy/untracked variant of `test_ci_squash_merge_mixed_content`.
#[test]
fn test_ci_squash_merge_mixed_content_standard_human() {
    let repo = direct_test_repo();
    let mut file = repo.filename("mixed.js");

    file.set_contents(crate::lines![
        "// Base code".unattributed_human(),
        "const base = 1;".unattributed_human()
    ]);
    let base_sha = repo
        .stage_all_and_commit("Initial commit")
        .unwrap()
        .commit_sha;
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(
        2,
        crate::lines![
            "// Untracked comment".unattributed_human(),
            "// AI generated function".ai(),
            "function aiHelper() {".ai(),
            "  return true;".ai(),
            "}".ai(),
            "// Another untracked comment".unattributed_human()
        ],
    );
    let head_sha = repo
        .stage_all_and_commit("Add mixed content")
        .unwrap()
        .commit_sha;

    let merge_sha = squash_feature_with_raw_git(&repo, "Merge feature via squash");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    file.assert_lines_and_blame(crate::lines![
        "// Base code".unattributed_human(),
        "const base = 1;".ai(),
        "// Untracked comment".ai(),
        "// AI generated function".ai(),
        "function aiHelper() {".ai(),
        "  return true;".ai(),
        "}".ai(),
        "// Another untracked comment".ai()
    ]);
}

/// Standard-human variant of `test_ci_squash_merge_with_manual_changes`.
#[test]
fn test_ci_squash_merge_with_manual_changes_standard_human() {
    let repo = direct_test_repo();
    let mut file = repo.filename("config.js");

    file.set_contents(crate::lines![
        "const config = {".unattributed_human(),
        "  version: 1".unattributed_human(),
        "};".unattributed_human()
    ]);
    let base_sha = repo
        .stage_all_and_commit("Initial commit")
        .unwrap()
        .commit_sha;
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.set_contents(crate::lines![
        "const config = {".unattributed_human(),
        "  version: 1,".ai(),
        "  // AI added feature flag".ai(),
        "  enableAI: true".ai(),
        "};".unattributed_human()
    ]);
    let head_sha = repo
        .stage_all_and_commit("Add AI config")
        .unwrap()
        .commit_sha;

    repo.git_og(&["checkout", "main"]).unwrap();
    repo.git_og(&["merge", "--squash", "feature"]).unwrap();
    file.set_contents(crate::lines![
        "const config = {".unattributed_human(),
        "  version: 1,".ai(),
        "  // AI added feature flag".unattributed_human(),
        "  enableAI: true,".unattributed_human(),
        "  // Manual addition during merge".unattributed_human(),
        "  production: false".unattributed_human(),
        "};".unattributed_human()
    ]);
    repo.git_og(&["add", "-A"]).unwrap();
    repo.git_og(&["commit", "-m", "Merge feature via squash with tweaks"])
        .unwrap();
    let merge_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    // Same new-logic tightening as test_ci_squash_merge_with_manual_changes:
    // `enableAI: true,` gained a trailing comma during the squash, so its
    // committed content differs from the AI checkpoint and it falls back to
    // untracked human. The leading untracked `version` line is recovered as
    // part of the AI edge.
    file.assert_lines_and_blame(crate::lines![
        "const config = {".unattributed_human(),
        "  version: 1,".ai(),
        "  // AI added feature flag".ai(),
        "  enableAI: true,".unattributed_human(),
        "  // Manual addition during merge".unattributed_human(),
        "  production: false".unattributed_human(),
        "};".unattributed_human()
    ]);
}

crate::reuse_tests_in_worktree!(
    test_ci_squash_merge_mixed_content,
    test_ci_squash_merge_with_manual_changes,
    test_ci_squash_merge_mixed_content_standard_human,
    test_ci_squash_merge_with_manual_changes_standard_human,
);
