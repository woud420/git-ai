//! Collection is opt-in: with an empty `allowed_repositories` list git-ai
//! collects nothing. TestRepo's default config patch allows the OS temp root,
//! so these tests override the allowlist explicitly where needed. Denied-repo
//! tests use dedicated daemons: the shared daemon's home config must not be
//! rewritten with an empty allowlist while other tests run against it.

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use std::fs;

#[test]
fn test_checkpoint_denied_with_empty_allowlist() {
    let mut repo = TestRepo::new_dedicated_daemon();
    repo.patch_git_ai_config(|patch| {
        patch.allowed_repositories = Some(vec![]);
    });

    fs::write(repo.path().join("file.txt"), "AI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file.txt"])
        .expect("denied checkpoint should exit successfully as a no-op");

    let working_logs = repo.path().join(".git/ai/working_logs");
    let entries: Vec<_> = match fs::read_dir(&working_logs) {
        Ok(dir) => dir.filter_map(Result::ok).map(|e| e.path()).collect(),
        Err(_) => vec![],
    };
    assert!(
        entries.is_empty(),
        "a denied repository must not get working log entries, found: {entries:?}"
    );
}

#[test]
fn test_commit_in_denied_repo_writes_no_authorship_note() {
    let mut repo = TestRepo::new_dedicated_daemon();
    repo.patch_git_ai_config(|patch| {
        patch.allowed_repositories = Some(vec![]);
    });

    fs::write(repo.path().join("file.txt"), "AI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file.txt"])
        .expect("denied checkpoint should exit successfully as a no-op");
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "-m", "commit in denied repo"])
        .unwrap();
    repo.sync_daemon();

    let head = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert!(
        repo.read_authorship_note(&head).is_none(),
        "a denied repository must not get authorship notes"
    );
}

#[test]
fn test_default_test_allowlist_allows_collection_via_path() {
    // TestRepo repos have no remotes; collection works because the default
    // test allowlist contains the OS temp root as a path entry.
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");
    file.set_contents(lines!["Human line", "AI line".ai()]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    file.assert_lines_and_blame(lines!["Human line".human(), "AI line".ai()]);
}

#[test]
fn test_reallowing_repo_restores_collection() {
    let mut repo = TestRepo::new_dedicated_daemon();
    repo.patch_git_ai_config(|patch| {
        patch.allowed_repositories = Some(vec![]);
    });

    let file_path = repo.path().join("example.txt");
    fs::write(&file_path, "Untracked line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .expect("denied checkpoint should exit successfully as a no-op");
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "-m", "denied commit"]).unwrap();
    repo.sync_daemon();
    let first = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert!(repo.read_authorship_note(&first).is_none());

    // Allow this repository by its root path and verify collection resumes.
    let repo_root = repo.canonical_path().to_string_lossy().replace('\\', "/");
    repo.patch_git_ai_config(move |patch| {
        patch.allowed_repositories = Some(vec![repo_root]);
    });

    let second_edit = "\
Untracked line
AI line
";
    fs::write(&file_path, second_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .expect("checkpoint should succeed once the repo is allowed");
    repo.stage_all_and_commit("allowed commit").unwrap();

    let mut file = repo.filename("example.txt");
    file.assert_committed_lines(lines![
        "Untracked line".unattributed_human(),
        "AI line".ai(),
    ]);
}

#[test]
fn test_env_var_allowlist_permits_collection_without_a_file_allowlist_entry() {
    // GIT_AI_ALLOWED_REPOSITORIES (CI/ephemeral-environment configuration
    // point) must union additively over the config file: it alone can make a
    // repo collectible even when the file-derived allowlist is empty. The env
    // var must reach both the `git-ai checkpoint` CLI (which pre-checks the
    // allowlist itself before ever talking to the daemon) and the daemon
    // (which decides whether to write authorship notes on commit) — each
    // reads its own process environment, so both need it explicitly.
    //
    // The value is deliberately the *raw*, non-canonicalized OS temp root
    // (on macOS this is a symlinked path, e.g. /var/folders/... ->
    // /private/var/folders/...) to exercise the same canonicalization trap
    // that `git-ai config --add allowed_repositories <path>` handles at
    // write time: repo roots are always matched in canonicalized form.
    let raw_temp_root = std::env::temp_dir().to_string_lossy().replace('\\', "/");
    let repo = TestRepo::new_with_daemon_env_and_patch(
        &[("GIT_AI_ALLOWED_REPOSITORIES", raw_temp_root.as_str())],
        |patch| {
            patch.allowed_repositories = Some(vec![]);
        },
    );

    let file_path = repo.path().join("example.txt");
    fs::write(&file_path, "AI line\n").unwrap();
    repo.git_ai_with_env(
        &["checkpoint", "mock_ai", "example.txt"],
        &[("GIT_AI_ALLOWED_REPOSITORIES", raw_temp_root.as_str())],
    )
    .expect("checkpoint should succeed: GIT_AI_ALLOWED_REPOSITORIES allows the repo");
    repo.stage_all_and_commit("env-allowed commit").unwrap();

    let mut file = repo.filename("example.txt");
    file.assert_committed_lines(lines!["AI line".ai()]);
}

#[test]
fn test_config_cli_accepts_canonical_and_legacy_allowlist_keys() {
    let repo = TestRepo::new();
    // Entries are validated: a path must point at an existing git repository.
    let repo_root = repo.canonical_path().to_string_lossy().replace('\\', "/");
    repo.git_ai(&["config", "--add", "allowed_repositories", &repo_root])
        .expect("adding an allowlist entry should succeed");

    // Read without the pre-invocation config sync: the sync rewrites
    // config.json from the test patch and would clobber the entry just added.
    let canonical = repo
        .git_ai_without_pre_sync_for_test(&["config", "allowed_repositories"])
        .expect("canonical key should be readable");
    assert!(
        canonical.contains(&repo_root),
        "expected added entry in: {canonical}"
    );

    let legacy = repo
        .git_ai_without_pre_sync_for_test(&["config", "allow_repositories"])
        .expect("legacy key should remain readable");
    assert!(
        legacy.contains(&repo_root),
        "expected added entry via legacy key in: {legacy}"
    );
}
