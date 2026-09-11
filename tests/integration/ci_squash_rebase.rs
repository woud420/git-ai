use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::authorship_log_serialization::AuthorshipLog;
use git_ai::operations::git::notes_api::write_note;
use git_ai::operations::git::repository as GitAiRepository;

fn direct_test_repo() -> TestRepo {
    TestRepo::new()
}

fn run_ci_local_merge(repo: &TestRepo, merge_sha: &str, head_sha: &str, base_sha: &str) -> String {
    repo.git_ai(&[
        "ci",
        "local",
        "merge",
        "--merge-commit-sha",
        merge_sha,
        "--base-ref",
        "main",
        "--head-ref",
        "feature",
        "--head-sha",
        head_sha,
        "--base-sha",
        base_sha,
        "--skip-fetch",
        "--skip-push",
    ])
    .expect("ci local merge should succeed")
}

fn assert_ci_rewrite_succeeded(output: &str) {
    assert!(
        output.contains("authorship rewritten successfully"),
        "expected ci local merge to rewrite authorship, got: {output}"
    );
}

fn authorship_files(repo: &TestRepo, commit_sha: &str) -> Vec<String> {
    repo.require_authorship_log(commit_sha)
        .attestations
        .iter()
        .map(|attestation| attestation.file_path.clone())
        .collect()
}

fn setup_main(repo: &TestRepo) -> String {
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    let base_sha = repo.stage_all_and_commit("base").unwrap().commit_sha;
    repo.git(&["branch", "-M", "main"]).unwrap();
    base_sha
}

fn squash_feature_with_raw_git(repo: &TestRepo, message: &str) -> String {
    repo.git_og(&["checkout", "main"]).unwrap();
    repo.git_og(&["merge", "--squash", "feature"]).unwrap();
    repo.git_og(&["commit", "-m", message]).unwrap();
    repo.git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

mod local_rebase_merge;
mod local_sync_and_open_pr;

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

#[test]
fn test_ci_rebase_merge_commit_order_pairing() {
    let repo = TestRepo::new();
    let base_sha = setup_main(&repo);

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut file_a = repo.filename("file_a.txt");
    file_a.set_contents(crate::lines!["ai content in file_a".ai()]);
    let feature_sha1 = repo.stage_all_and_commit("add file_a").unwrap().commit_sha;

    let mut file_b = repo.filename("file_b.txt");
    file_b.set_contents(crate::lines!["ai content in file_b".ai()]);
    let feature_sha2 = repo.stage_all_and_commit("add file_b").unwrap().commit_sha;

    repo.git_og(&["checkout", "main"]).unwrap();
    let mut main_only = repo.filename("main_only.txt");
    main_only.set_contents(crate::lines!["main-only content"]);
    repo.git_og(&["add", "main_only.txt"]).unwrap();
    repo.git_og(&["commit", "-m", "advance main"]).unwrap();

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

    assert_ne!(new_sha1, feature_sha1);
    assert_ne!(new_sha2, feature_sha2);

    repo.git_og(&["checkout", "main"]).unwrap();
    repo.git_og(&["merge", "--ff-only", "feature"]).unwrap();

    let output = run_ci_local_merge(&repo, &new_sha2, &feature_sha2, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    let files1 = authorship_files(&repo, &new_sha1);
    let files2 = authorship_files(&repo, &new_sha2);

    assert!(
        files1.iter().any(|file| file.contains("file_a")),
        "rebased commit 1 should reference file_a.txt, got: {files1:?}"
    );
    assert!(
        !files1.iter().any(|file| file.contains("file_b")),
        "rebased commit 1 should not reference file_b.txt, got: {files1:?}"
    );
    assert!(
        files2.iter().any(|file| file.contains("file_b")),
        "rebased commit 2 should reference file_b.txt, got: {files2:?}"
    );
    assert!(
        !files2.iter().any(|file| file.contains("file_a")),
        "rebased commit 2 should not reference file_a.txt, got: {files2:?}"
    );
}

/// Multi-commit feature (AI + AI + human) squashed into one merge commit: the
/// squashed commit splits attribution across authors. Originally exercised the
/// removed engine directly.
#[test]
fn test_ci_rebase_merge_multiple_commits() {
    let repo = direct_test_repo();
    let mut file = repo.filename("app.js");

    file.set_contents(crate::lines!["// App v1", ""]);
    let base_sha = repo
        .stage_all_and_commit("Initial commit")
        .unwrap()
        .commit_sha;
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(
        1,
        crate::lines!["// AI function 1".ai(), "function ai1() { }".ai()],
    );
    repo.stage_all_and_commit("Add AI function 1").unwrap();
    file.insert_at(
        3,
        crate::lines!["// AI function 2".ai(), "function ai2() { }".ai()],
    );
    repo.stage_all_and_commit("Add AI function 2").unwrap();
    file.insert_at(
        5,
        crate::lines!["// Human function", "function human() { }"],
    );
    let head_sha = repo
        .stage_all_and_commit("Add human function")
        .unwrap()
        .commit_sha;

    let merge_sha = squash_feature_with_raw_git(&repo, "Merge feature branch (squashed)");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    file.assert_lines_and_blame(crate::lines![
        "// App v1".human(),
        "// AI function 1".ai(),
        "function ai1() { }".ai(),
        "// AI function 2".ai(),
        "function ai2() { }".ai(),
        "// Human function".human(),
        "function human() { }".human()
    ]);
}

/// Standard-human variant of `test_ci_rebase_merge_multiple_commits`.
#[test]
fn test_ci_rebase_merge_multiple_commits_standard_human() {
    let repo = direct_test_repo();
    let mut file = repo.filename("app.js");

    file.set_contents(crate::lines![
        "// App v1".unattributed_human(),
        "".unattributed_human()
    ]);
    let base_sha = repo
        .stage_all_and_commit("Initial commit")
        .unwrap()
        .commit_sha;
    repo.git(&["branch", "-M", "main"]).unwrap();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    file.insert_at(
        1,
        crate::lines!["// AI function 1".ai(), "function ai1() { }".ai()],
    );
    repo.stage_all_and_commit("Add AI function 1").unwrap();
    file.insert_at(
        3,
        crate::lines!["// AI function 2".ai(), "function ai2() { }".ai()],
    );
    repo.stage_all_and_commit("Add AI function 2").unwrap();
    file.insert_at(
        5,
        crate::lines![
            "// Human function".unattributed_human(),
            "function human() { }".unattributed_human()
        ],
    );
    let head_sha = repo
        .stage_all_and_commit("Add human function")
        .unwrap()
        .commit_sha;

    let merge_sha = squash_feature_with_raw_git(&repo, "Merge feature branch (squashed)");
    let output = run_ci_local_merge(&repo, &merge_sha, &head_sha, &base_sha);
    assert_ci_rewrite_succeeded(&output);

    file.assert_lines_and_blame(crate::lines![
        "// App v1".unattributed_human(),
        "// AI function 1".ai(),
        "function ai1() { }".ai(),
        "// AI function 2".ai(),
        "function ai2() { }".ai(),
        "// Human function".unattributed_human(),
        "function human() { }".unattributed_human()
    ]);
}

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
    test_ci_squash_merge_mixed_content,
    test_ci_squash_merge_with_manual_changes,
    test_ci_squash_merge_mixed_content_standard_human,
    test_ci_squash_merge_with_manual_changes_standard_human,
    test_ci_local_merge_squash_on_linear_main_does_not_note_base_commits,
    test_ci_squash_merge_not_misclassified_as_rebase_on_linear_main,
    test_ci_rebase_merge_commit_order_pairing,
    test_ci_rebase_merge_multiple_commits,
    test_ci_rebase_merge_multiple_commits_standard_human,
    test_ci_squash_merge_basic,
    test_ci_squash_merge_multiple_files,
    test_ci_squash_merge_mixed_ai_and_human_content,
    test_ci_squash_merge_no_notes_no_authorship_created,
    test_ci_squash_merge_empty_notes_preserved,
    test_ci_squash_merge_basic_standard_human,
);
