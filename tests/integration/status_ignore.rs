use crate::repos::diff_hostility::{
    configure_hostile_diff_settings, configure_repo_external_diff_helper,
};
use crate::repos::test_repo::TestRepo;
use crate::test_utils::extract_json_object;
use git_ai::operations::authorship::stats::CommitStats;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct StatusOutput {
    stats: CommitStats,
    checkpoints: Vec<serde_json::Value>,
}

fn status_from_args(repo: &TestRepo, args: &[&str]) -> StatusOutput {
    let raw = repo.git_ai(args).expect("git-ai status should succeed");
    let json = extract_json_object(&raw);
    serde_json::from_str(&json).expect("valid status json")
}

#[test]
fn test_checkpoint_ignores_default_lockfiles_integration() {
    let repo = TestRepo::new();

    repo.write_file("README.md", "# repo\n");
    repo.stage_all_and_commit("initial").unwrap();

    repo.write_file("README.md", "# repo\nupdated\n");
    repo.write_file("Cargo.lock", "# lock\n# lock2\n# lock3\n");

    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    let checkpoints = repo.current_working_logs().read_all_checkpoints().unwrap();
    let latest = checkpoints.last().expect("checkpoint should be present");

    assert!(
        latest.entries.iter().any(|entry| entry.file == "README.md"),
        "Expected non-ignored file to be checkpointed"
    );
    assert!(
        latest
            .entries
            .iter()
            .all(|entry| entry.file != "Cargo.lock"),
        "Expected Cargo.lock to be filtered out by default ignores"
    );
}

#[test]
fn test_checkpoint_honors_uncommitted_root_gitattributes_linguist_generated_integration() {
    let repo = TestRepo::new();

    repo.write_file("src/main.rs", "fn main() {}\n");
    repo.stage_all_and_commit("initial").unwrap();

    repo.write_file(".gitattributes", "generated/** linguist-generated=true\n");
    repo.write_file("src/main.rs", "fn main() {}\nfn added() {}\n");
    repo.write_file(
        "generated/api.generated.ts",
        "export const one = 1;\nexport const two = 2;\n",
    );

    repo.git_ai(&[
        "checkpoint",
        "mock_ai",
        "src/main.rs",
        "generated/api.generated.ts",
    ])
    .unwrap();

    let checkpoints = repo.current_working_logs().read_all_checkpoints().unwrap();
    let latest = checkpoints.last().expect("checkpoint should be present");

    assert!(
        latest
            .entries
            .iter()
            .any(|entry| entry.file == "src/main.rs"),
        "Expected regular source file to be checkpointed"
    );
    assert!(
        latest
            .entries
            .iter()
            .all(|entry| entry.file != "generated/api.generated.ts"),
        "Expected linguist-generated file to be filtered out"
    );
}

#[test]
fn test_status_default_ignores_affect_git_diff_and_ai_accepted() {
    let repo = TestRepo::new();

    repo.write_file("README.md", "# repo\n");
    repo.stage_all_and_commit("initial").unwrap();

    repo.write_file("README.md", "# repo\nnew ai line\n");
    repo.write_file("Cargo.lock", "# lock\n# lock2\n# lock3\n");

    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    let status = status_from_args(&repo, &["status", "--json"]);

    assert_eq!(status.stats.git_diff_added_lines, 1);
    assert_eq!(status.stats.git_diff_deleted_lines, 0);
    assert_eq!(status.stats.ai_accepted, 1);
    assert!(
        !status.checkpoints.is_empty(),
        "status should report at least one checkpoint"
    );
}

#[test]
fn test_status_honors_uncommitted_root_gitattributes_linguist_generated() {
    let repo = TestRepo::new();

    repo.write_file("src/app.ts", "export const app = 1;\n");
    repo.stage_all_and_commit("initial").unwrap();

    repo.write_file(".gitattributes", "generated/** linguist-generated=true\n");
    repo.write_file(
        "src/app.ts",
        "export const app = 1;\nexport const next = 2;\n",
    );
    repo.write_file(
        "generated/out.generated.ts",
        "export const generatedA = 1;\nexport const generatedB = 2;\n",
    );

    repo.git_ai(&[
        "checkpoint",
        "mock_ai",
        "src/app.ts",
        "generated/out.generated.ts",
    ])
    .unwrap();

    let status = status_from_args(&repo, &["status", "--json"]);

    assert_eq!(status.stats.git_diff_added_lines, 1);
    assert_eq!(status.stats.git_diff_deleted_lines, 0);
    assert_eq!(status.stats.ai_accepted, 1);
}

#[test]
fn test_status_with_only_ignored_changes_reports_zero_diff() {
    let repo = TestRepo::new();

    repo.write_file("README.md", "# repo\n");
    repo.stage_all_and_commit("initial").unwrap();

    repo.write_file("Cargo.lock", "# lock\n# lock2\n# lock3\n");

    let status = status_from_args(&repo, &["status", "--json"]);

    assert_eq!(status.stats.git_diff_added_lines, 0);
    assert_eq!(status.stats.git_diff_deleted_lines, 0);
    assert_eq!(status.stats.ai_accepted, 0);
}

#[test]
fn test_checkpoint_honors_git_ai_ignore_file() {
    let repo = TestRepo::new();

    repo.write_file("src/main.rs", "fn main() {}\n");
    repo.stage_all_and_commit("initial").unwrap();

    repo.write_file(".git-ai-ignore", "docs/**\n");
    repo.write_file("src/main.rs", "fn main() {}\nfn added() {}\n");
    repo.write_file("docs/guide.md", "# Guide\nLine 1\nLine 2\n");

    repo.git_ai(&["checkpoint", "mock_ai", "src/main.rs", "docs/guide.md"])
        .unwrap();

    let checkpoints = repo.current_working_logs().read_all_checkpoints().unwrap();
    let latest = checkpoints.last().expect("checkpoint should be present");

    assert!(
        latest
            .entries
            .iter()
            .any(|entry| entry.file == "src/main.rs"),
        "Expected regular source file to be checkpointed"
    );
    assert!(
        latest
            .entries
            .iter()
            .all(|entry| entry.file != "docs/guide.md"),
        "Expected .git-ai-ignore pattern to filter out docs/guide.md"
    );
}

#[test]
fn test_status_honors_git_ai_ignore_file() {
    let repo = TestRepo::new();

    repo.write_file("src/app.ts", "export const app = 1;\n");
    repo.stage_all_and_commit("initial").unwrap();

    repo.write_file(".git-ai-ignore", "docs/**\n");
    repo.write_file(
        "src/app.ts",
        "export const app = 1;\nexport const next = 2;\n",
    );
    repo.write_file("docs/api.md", "# API\nendpoint 1\nendpoint 2\n");

    repo.git_ai(&["checkpoint", "mock_ai", "src/app.ts", "docs/api.md"])
        .unwrap();

    let status = status_from_args(&repo, &["status", "--json"]);

    assert_eq!(status.stats.git_diff_added_lines, 1);
    assert_eq!(status.stats.git_diff_deleted_lines, 0);
    assert_eq!(status.stats.ai_accepted, 1);
}

#[test]
fn test_status_git_ai_ignore_union_with_gitattributes() {
    let repo = TestRepo::new();

    repo.write_file("src/app.ts", "export const app = 1;\n");
    repo.stage_all_and_commit("initial").unwrap();

    // Set up both .gitattributes and .git-ai-ignore
    repo.write_file(".gitattributes", "generated/** linguist-generated=true\n");
    repo.write_file(".git-ai-ignore", "docs/**\n");
    repo.write_file(
        "src/app.ts",
        "export const app = 1;\nexport const next = 2;\n",
    );
    repo.write_file(
        "generated/out.ts",
        "export const gen = 1;\nexport const gen2 = 2;\n",
    );
    repo.write_file("docs/api.md", "# API\nendpoint 1\nendpoint 2\n");

    repo.git_ai(&[
        "checkpoint",
        "mock_ai",
        "src/app.ts",
        "generated/out.ts",
        "docs/api.md",
    ])
    .unwrap();

    let status = status_from_args(&repo, &["status", "--json"]);

    // Only src/app.ts addition should be counted (1 line)
    // generated/out.ts ignored by .gitattributes linguist-generated
    // docs/api.md ignored by .git-ai-ignore
    assert_eq!(status.stats.git_diff_added_lines, 1);
    assert_eq!(status.stats.git_diff_deleted_lines, 0);
    assert_eq!(status.stats.ai_accepted, 1);
}

#[test]
fn test_status_ignores_repo_external_diff_helper_for_internal_numstat() {
    let repo = TestRepo::new();

    repo.write_file("app.txt", "line1\n");
    repo.stage_all_and_commit("initial").unwrap();

    repo.write_file("app.txt", "line1\nline2\n");
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    let marker = configure_repo_external_diff_helper(
        &repo,
        "STATUS_EXTERNAL_DIFF_MARKER",
        "status-ext-diff-helper.sh",
    );
    let proxied_diff = repo
        .git(&["diff", "HEAD"])
        .expect("proxied git diff should succeed");
    assert!(
        proxied_diff.contains(&marker),
        "sanity check: proxied git diff should use configured external helper"
    );

    let status = status_from_args(&repo, &["status", "--json"]);
    assert_eq!(status.stats.git_diff_added_lines, 1);
    assert_eq!(status.stats.git_diff_deleted_lines, 0);
    assert_eq!(status.stats.ai_accepted, 1);
}

#[test]
fn test_status_numstat_is_stable_under_hostile_diff_config() {
    let repo = TestRepo::new();

    repo.write_file("app.txt", "line1\n");
    repo.stage_all_and_commit("initial").unwrap();

    repo.write_file("app.txt", "line1\nline2\n");
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
    configure_hostile_diff_settings(&repo);

    let status = status_from_args(&repo, &["status", "--json"]);
    assert_eq!(status.stats.git_diff_added_lines, 1);
    assert_eq!(status.stats.git_diff_deleted_lines, 0);
    assert_eq!(status.stats.ai_accepted, 1);
}
crate::reuse_tests_in_worktree!(
    test_checkpoint_ignores_default_lockfiles_integration,
    test_checkpoint_honors_uncommitted_root_gitattributes_linguist_generated_integration,
    test_status_default_ignores_affect_git_diff_and_ai_accepted,
    test_status_honors_uncommitted_root_gitattributes_linguist_generated,
    test_status_with_only_ignored_changes_reports_zero_diff,
    test_checkpoint_honors_git_ai_ignore_file,
    test_status_honors_git_ai_ignore_file,
    test_status_git_ai_ignore_union_with_gitattributes,
    test_status_ignores_repo_external_diff_helper_for_internal_numstat,
    test_status_numstat_is_stable_under_hostile_diff_config,
);
