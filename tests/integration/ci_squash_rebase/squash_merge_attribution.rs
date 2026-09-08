use super::{
    AuthorshipLog, ExpectedLineExt, TestRepo, assert_ci_rewrite_succeeded, direct_test_repo,
    run_ci_local_merge, setup_main, squash_feature_with_raw_git,
};

#[test]
fn test_ci_squash_merge_basic() {
    let repo = TestRepo::new();
    let base_sha = setup_main(&repo);

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature = repo.filename("feature.js");
    feature.set_contents(crate::lines![
        "export function aiFeature() {".ai(),
        "  return 'ai code';".ai(),
        "}".ai()
    ]);
    let head_sha = repo
        .stage_all_and_commit("add ai feature")
        .unwrap()
        .commit_sha;

    let merge_sha = squash_feature_with_raw_git(&repo, "squash feature");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    feature.assert_lines_and_blame(crate::lines![
        "export function aiFeature() {".ai(),
        "  return 'ai code';".ai(),
        "}".ai()
    ]);
}

#[test]
fn test_ci_squash_merge_multiple_files() {
    let repo = TestRepo::new();
    let base_sha = setup_main(&repo);

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut api = repo.filename("api.js");
    let mut view = repo.filename("view.js");
    api.set_contents(crate::lines![
        "export const handler = () => {".ai(),
        "  return 'ok';".ai(),
        "};".ai()
    ]);
    view.set_contents(crate::lines![
        "export function View() {".ai(),
        "  return handler();".ai(),
        "}".ai()
    ]);
    let head_sha = repo
        .stage_all_and_commit("add ai feature files")
        .unwrap()
        .commit_sha;

    let merge_sha = squash_feature_with_raw_git(&repo, "squash feature files");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    api.assert_lines_and_blame(crate::lines![
        "export const handler = () => {".ai(),
        "  return 'ok';".ai(),
        "};".ai()
    ]);
    view.assert_lines_and_blame(crate::lines![
        "export function View() {".ai(),
        "  return handler();".ai(),
        "}".ai()
    ]);
}

#[test]
fn test_ci_squash_merge_mixed_ai_and_human_content() {
    let repo = TestRepo::new();
    let base_sha = setup_main(&repo);

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut mixed = repo.filename("mixed.js");
    mixed.set_contents(crate::lines![
        "// Human-written setup",
        "const flag = true;",
        "// AI generated helper".ai(),
        "function helper() {".ai(),
        "  return flag;".ai(),
        "}".ai(),
        "// Human-written footer"
    ]);
    let head_sha = repo
        .stage_all_and_commit("add mixed feature")
        .unwrap()
        .commit_sha;

    let merge_sha = squash_feature_with_raw_git(&repo, "squash mixed feature");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    mixed.assert_lines_and_blame(crate::lines![
        "// Human-written setup".human(),
        "const flag = true;".human(),
        "// AI generated helper".ai(),
        "function helper() {".ai(),
        "  return flag;".ai(),
        "}".ai(),
        "// Human-written footer".human()
    ]);
}

#[test]
fn test_ci_squash_merge_no_notes_no_authorship_created() {
    let repo = TestRepo::new();

    let file_path = repo.path().join("feature.txt");
    std::fs::write(&file_path, "base\n").unwrap();
    repo.git_og(&["add", "feature.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "base"]).unwrap();
    let base_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    repo.git_og(&["branch", "-M", "main"]).unwrap();

    repo.git_og(&["checkout", "-b", "feature"]).unwrap();
    std::fs::write(&file_path, "base\nhuman change\n").unwrap();
    repo.git_og(&["commit", "-am", "human feature"]).unwrap();
    let head_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let merge_sha = squash_feature_with_raw_git(&repo, "squash human feature");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);

    assert!(
        output.contains("no AI authorship to track"),
        "expected ci local merge to report no authorship, got: {output}"
    );
    assert!(
        repo.read_authorship_note(&merge_sha).is_none(),
        "expected no authorship note when source commits have no notes"
    );
}

/// Squash merge where the feature's source commits carry notes but no AI
/// attestations (human-only change): the squashed commit must end up with no AI
/// prompts. Originally exercised the removed engine directly.
#[test]
fn test_ci_squash_merge_empty_notes_preserved() {
    let repo = direct_test_repo();
    let mut file = repo.filename("feature.txt");

    file.set_contents(crate::lines!["base"]);
    let base_sha = repo
        .stage_all_and_commit("Initial commit")
        .unwrap()
        .commit_sha;
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.set_contents(crate::lines!["base", "human change"]);
    let head_sha = repo
        .stage_all_and_commit("Human change")
        .unwrap()
        .commit_sha;

    let merge_sha = squash_feature_with_raw_git(&repo, "Merge feature via squash");
    let _ = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);

    // Human-only squash: if a note exists it must carry no AI prompts.
    if let Some(note) = repo.read_authorship_note(&merge_sha) {
        let log = AuthorshipLog::deserialize_from_string(&note).unwrap();
        assert!(
            log.metadata.prompts.is_empty(),
            "human-only squash merge must not produce AI prompts, got: {:?}",
            log.metadata.prompts
        );
    }
}

/// Standard-human variant of `test_ci_squash_merge_basic`: original lines are
/// untracked human (checkpoint `human`) rather than known-human.
#[test]
fn test_ci_squash_merge_basic_standard_human() {
    let repo = direct_test_repo();
    let mut file = repo.filename("feature.js");

    file.set_contents(crate::lines![
        "// Original code".unattributed_human(),
        "function original() {}".unattributed_human()
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
            "// AI added function".ai(),
            "function aiFeature() {".ai(),
            "  return 'ai code';".ai(),
            "}".ai()
        ],
    );
    let head_sha = repo
        .stage_all_and_commit("Add AI feature")
        .unwrap()
        .commit_sha;

    let merge_sha = squash_feature_with_raw_git(&repo, "Merge feature via squash");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    file.assert_lines_and_blame(crate::lines![
        "// Original code".unattributed_human(),
        "function original() {}".ai(),
        "// AI added function".ai(),
        "function aiFeature() {".ai(),
        "  return 'ai code';".ai(),
        "}".ai()
    ]);
}

crate::reuse_tests_in_worktree!(
    test_ci_squash_merge_basic,
    test_ci_squash_merge_multiple_files,
    test_ci_squash_merge_mixed_ai_and_human_content,
    test_ci_squash_merge_no_notes_no_authorship_created,
    test_ci_squash_merge_empty_notes_preserved,
    test_ci_squash_merge_basic_standard_human,
);
