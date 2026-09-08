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

mod human_squash_resolution;
mod linear_main;
mod local_rebase_merge;
mod local_sync_and_open_pr;
mod rebase_merge;
mod squash_merge_attribution;
