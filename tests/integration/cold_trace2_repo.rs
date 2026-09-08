use git_ai::model::authorship_log_serialization::AuthorshipLog;

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use std::fs;

const TRACE2_DISABLED_ENV: [(&str, &str); 3] = [
    ("GIT_TRACE2", "0"),
    ("GIT_TRACE2_EVENT", "0"),
    ("GIT_TRACE2_PERF", "0"),
];

fn cold_repo() -> TestRepo {
    TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon)
}

fn raw_git(repo: &TestRepo, args: &[&str]) -> String {
    repo.git_og_with_env(args, &TRACE2_DISABLED_ENV)
        .unwrap_or_else(|error| panic!("raw trace-disabled git {:?} failed: {}", args, error))
}

fn raw_git_result(repo: &TestRepo, args: &[&str]) -> Result<String, String> {
    repo.git_og_with_env(args, &TRACE2_DISABLED_ENV)
}

fn raw_head(repo: &TestRepo) -> String {
    raw_git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

fn raw_commit_all(repo: &TestRepo, message: &str) -> String {
    raw_git(repo, &["add", "-A"]);
    raw_git(repo, &["commit", "-m", message]);
    raw_head(repo)
}

fn raw_commit_file(repo: &TestRepo, path: &str, content: &str, message: &str) -> String {
    repo.write_file(path, content);
    raw_commit_all(repo, message)
}

fn raw_clone(source: &TestRepo, target_path: &std::path::Path) -> TestRepo {
    raw_git(
        source,
        &[
            "clone",
            source.path().to_str().expect("source path should be utf-8"),
            target_path.to_str().expect("target path should be utf-8"),
        ],
    );
    TestRepo::new_at_path_with_daemon_scope(target_path, DaemonTestScope::NoDaemon)
}

fn traced_ai_commit_file(repo: &TestRepo, path: &str, content: &str, message: &str) -> String {
    repo.write_file(path, content);
    repo.git_ai(&["checkpoint", "mock_ai", path])
        .unwrap_or_else(|error| panic!("mock_ai checkpoint for {} failed: {}", path, error));
    repo.stage_all_and_commit(message)
        .unwrap_or_else(|error| panic!("commit {} failed: {}", message, error))
        .commit_sha
}

fn read_file(repo: &TestRepo, path: &str) -> String {
    fs::read_to_string(repo.path().join(path)).unwrap()
}

fn start_cold_daemon(repo: &mut TestRepo) {
    repo.start_dedicated_daemon_for_test();
}

fn run_traced_git(repo: &TestRepo, args: &[&str]) -> String {
    let output = run_traced_git_without_sync(repo, args);
    repo.sync_daemon_force();
    output
}

fn run_traced_git_without_sync(repo: &TestRepo, args: &[&str]) -> String {
    assert!(
        repo.git_command_affects_daemon_for_tracking(args, None),
        "git {:?} should be tracked by daemon test sync",
        args
    );
    repo.git(args)
        .unwrap_or_else(|error| panic!("traced git {:?} failed: {}", args, error))
}

fn assert_ai_authorship_note(repo: &TestRepo, commit_sha: &str) {
    let log = repo.require_authorship_log(commit_sha);
    assert!(
        log.attestations
            .iter()
            .any(|attestation| !attestation.entries.is_empty()),
        "commit {commit_sha} should contain AI authorship entries"
    );
}

fn assert_no_ai_authorship_for_commit(repo: &TestRepo, commit_sha: &str) {
    let Some(note) = repo.read_authorship_note(commit_sha) else {
        return;
    };
    assert_note_has_no_ai_authorship(commit_sha, &note);
}

fn assert_no_authorship_note(repo: &TestRepo, commit_sha: &str) {
    assert!(
        repo.read_authorship_note(commit_sha).is_none(),
        "commit {commit_sha} should not have an authorship note"
    );
}

fn assert_traced_commit_has_no_ai_authorship(repo: &TestRepo, commit_sha: &str) {
    let note = repo
        .read_authorship_note(commit_sha)
        .unwrap_or_else(|| panic!("traced commit {commit_sha} should have an authorship note"));
    assert_note_has_no_ai_authorship(commit_sha, &note);
}

fn assert_note_has_no_ai_authorship(commit_sha: &str, note: &str) {
    let log = AuthorshipLog::deserialize_from_string(note)
        .unwrap_or_else(|error| panic!("failed to parse authorship note: {}", error));
    assert!(
        log.attestations
            .iter()
            .all(|attestation| attestation.entries.is_empty()),
        "cold raw setup should not create attestations for {}: {:?}",
        commit_sha,
        log.attestations
    );
    assert!(
        log.metadata.prompts.is_empty() && log.metadata.sessions.is_empty(),
        "cold raw setup should not create AI metadata for {}: {:?}",
        commit_sha,
        log.metadata
    );
}

fn run_cold_repo_first_traced_pull_rebase_preserves_rebased_ai_authorship() {
    let upstream = TestRepo::new_bare_with_daemon_scope(DaemonTestScope::NoDaemon);
    raw_git(&upstream, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let mut repo = cold_repo();
    raw_git(&repo, &["branch", "-M", "main"]);
    raw_git(
        &repo,
        &["remote", "add", "origin", upstream.path().to_str().unwrap()],
    );
    raw_commit_file(&repo, "README.md", "# Test Repo\n", "raw initial");
    raw_git(&repo, &["push", "-u", "origin", "HEAD:main"]);

    start_cold_daemon(&mut repo);
    let local_ai_commit = traced_ai_commit_file(
        &repo,
        "ai_feature.txt",
        "AI generated feature line 1\nAI generated feature line 2\n",
        "add AI feature",
    );
    assert_ai_authorship_note(&repo, &local_ai_commit);

    let contributor_parent = tempfile::tempdir().expect("contributor temp dir");
    let contributor_path = contributor_parent.path().join("contributor");
    let contributor = raw_clone(&upstream, &contributor_path);
    raw_git(&contributor, &["checkout", "main"]);
    raw_commit_file(
        &contributor,
        "upstream_change.txt",
        "upstream content\n",
        "upstream divergent commit",
    );
    raw_git(&contributor, &["push", "origin", "HEAD:main"]);

    assert!(
        repo.git(&["push"]).is_err(),
        "push should be rejected because origin has diverged"
    );
    assert!(
        repo.git(&["pull"]).is_err(),
        "plain pull should fail before an explicit reconciliation strategy"
    );
    repo.git(&["pull", "--rebase"])
        .expect("pull --rebase should succeed");
    repo.sync_daemon_force();

    let rebased = raw_head(&repo);
    assert_ne!(rebased, local_ai_commit);
    assert_ai_authorship_note(&repo, &rebased);
}

mod first_traced_commands;
mod merge_and_stash;
mod rebase_recovery;
