//! The `git` wrapper (`handle_git`) only discovers the repository when a
//! successful commit is about to show post-commit stats; other mutating
//! commands are proxied without discovery's `git rev-parse` spawns.

use crate::repos::test_repo::TestRepo;
use std::fs;

fn run_wrapper(repo: &TestRepo, args: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let mut wrapper_envs = vec![("GIT_AI", "git")];
    wrapper_envs.extend_from_slice(envs);
    repo.git_ai_command_without_pre_sync_for_test(args, &wrapper_envs)
        .output()
        .expect("git wrapper should run")
}

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn wrapper_proxies_mutating_non_commit_command_without_repository_discovery() {
    let repo = TestRepo::new();
    fs::write(repo.path().join("a.txt"), "a\n").unwrap();
    let spawn_log = repo.test_home_path().join("wrapper-add-spawns.log");

    let output = run_wrapper(
        &repo,
        &["add", "a.txt"],
        &[("GIT_AI_SPAWN_LOG", spawn_log.to_str().unwrap())],
    );
    assert!(output.status.success(), "{}", combined(&output));

    let staged = repo.git_og(&["diff", "--cached", "--name-only"]).unwrap();
    assert_eq!(staged.trim(), "a.txt");
    let spawns = fs::read_to_string(&spawn_log).unwrap_or_default();
    assert!(
        !spawns.lines().any(|command| command == "rev-parse"),
        "`git add` through the wrapper must not discover the repository; spawns: {spawns:?}"
    );
}

#[test]
fn wrapper_commit_discovers_repository_for_post_commit_stats() {
    let repo = TestRepo::new();
    fs::write(repo.path().join("a.txt"), "a\n").unwrap();
    repo.git_og(&["add", "a.txt"]).unwrap();
    // An empty notes db and a zero timeout make the note lookup miss once,
    // so the wrapper deterministically reports the commit as still processing.
    let notes_db_dir = tempfile::tempdir().unwrap();
    let notes_db_path = notes_db_dir.path().join("notes-db");

    let output = run_wrapper(
        &repo,
        &["commit", "-m", "first"],
        &[
            ("GIT_AI_TEST_FORCE_TTY", "1"),
            ("GIT_AI_POST_COMMIT_TIMEOUT_MS", "0"),
            ("GIT_AI_TEST_NOTES_DB_PATH", notes_db_path.to_str().unwrap()),
        ],
    );
    let output_text = combined(&output);
    assert!(output.status.success(), "{output_text}");

    let head = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    let expected = format!("[git-ai] still processing commit {}", &head.trim()[..8]);
    assert!(
        output_text.contains(&expected),
        "expected {expected:?} in wrapper output:\n{output_text}"
    );
}

// Linux only: macOS file systems refuse to create a non-UTF-8 file name.
#[cfg(target_os = "linux")]
#[test]
fn wrapper_passes_non_utf8_arguments_through_to_git() {
    use std::os::unix::ffi::OsStrExt;

    let repo = TestRepo::new();
    // A Latin-1 file name: valid on disk and for git, but not UTF-8.
    let name = std::ffi::OsStr::from_bytes(b"caf\xe9.txt");
    fs::write(repo.path().join(name), "a\n").unwrap();

    let output = repo
        .git_ai_command_without_pre_sync_for_test(&["add"], &[("GIT_AI", "git")])
        .arg(name)
        .output()
        .expect("git wrapper should run");
    assert!(output.status.success(), "{}", combined(&output));

    let staged = repo.git_og(&["diff", "--cached", "--name-only"]).unwrap();
    assert_eq!(staged.trim(), r#""caf\351.txt""#);
}

#[test]
fn wrapper_does_not_swallow_a_leading_double_dash() {
    let repo = TestRepo::new();

    // Real git rejects `git -- version`; the wrapper must not turn it into
    // `git version`.
    let output = run_wrapper(&repo, &["--", "version"], &[]);
    let output_text = combined(&output);
    assert!(!output.status.success(), "{output_text}");
    assert!(output_text.contains("unknown option: --"), "{output_text}");
}
