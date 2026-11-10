use super::{
    GitBackend, SystemGitBackend, clone_init_positionals, default_clone_target_from_source,
};
use std::fs;
use std::path::PathBuf;

fn argv(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

// --- Bug: `takes_value` is incomplete — options like --depth, -j, -c are not listed.
// When `git clone --depth 1 <url>` is parsed, "1" is treated as a positional arg and
// the URL ends up as positional[1] (the "target directory"), triggering the error:
//   "failed to resolve clone/init target family from filesystem: <url>"
//
// These tests pin the CORRECT behaviour (URL-derived name as the only positional)
// and will FAIL until `takes_value` includes those options.

#[test]
fn clone_positionals_skips_value_for_depth_flag() {
    let args = argv(&["--depth", "1", "https://example.com/org/test-repo.git"]);
    assert_eq!(
        clone_init_positionals(&args),
        vec!["https://example.com/org/test-repo.git".to_string()],
        "--depth should consume its value, leaving only the URL as a positional"
    );
}

#[test]
fn clone_positionals_skips_value_for_jobs_short_flag() {
    let args = argv(&["-j", "4", "https://example.com/org/test-repo.git"]);
    assert_eq!(
        clone_init_positionals(&args),
        vec!["https://example.com/org/test-repo.git".to_string()],
        "-j should consume its value, leaving only the URL as a positional"
    );
}

#[test]
fn clone_positionals_skips_value_for_jobs_long_flag() {
    let args = argv(&["--jobs", "4", "https://example.com/org/test-repo.git"]);
    assert_eq!(
        clone_init_positionals(&args),
        vec!["https://example.com/org/test-repo.git".to_string()],
        "--jobs should consume its value, leaving only the URL as a positional"
    );
}

#[test]
fn clone_positionals_skips_value_for_config_short_flag() {
    let args = argv(&[
        "-c",
        "http.sslVerify=false",
        "https://example.com/org/test-repo.git",
    ]);
    assert_eq!(
        clone_init_positionals(&args),
        vec!["https://example.com/org/test-repo.git".to_string()],
        "-c should consume its value, leaving only the URL as a positional"
    );
}

#[test]
fn clone_target_derives_name_from_url_with_depth_flag() {
    let backend = SystemGitBackend::new();
    let cwd = PathBuf::from("/home/testuser/projects");
    let args = argv(&[
        "git",
        "clone",
        "--depth",
        "1",
        "https://example.com/org/test-repo.git",
    ]);
    let result = backend.clone_target(&args, Some(&cwd)).unwrap();
    assert_eq!(
        result,
        PathBuf::from("/home/testuser/projects/test-repo"),
        "clone target should be derived from the URL, not the depth value"
    );
}

// --- Bug: when `cwd_hint` is None and the derived target is relative (e.g. "." for
// `git init` with no args), the path cannot be resolved and filesystem checks run
// against the daemon's own CWD rather than the actual target, producing the error:
//   "failed to resolve clone/init target family from filesystem: ."
//
// These tests pin the CORRECT behaviour (return None so that the caller doesn't
// attempt a meaningless filesystem lookup against an unresolvable relative path).

#[test]
fn init_target_returns_none_for_implicit_dot_without_cwd_hint() {
    let backend = SystemGitBackend::new();
    let args = argv(&["git", "init"]);
    assert!(
        backend.init_target(&args, None).is_none(),
        "init with no path and no cwd_hint should return None — \
             relative '.' cannot be reliably resolved"
    );
}

#[test]
fn clone_target_returns_none_for_explicit_dot_without_cwd_hint() {
    let backend = SystemGitBackend::new();
    let args = argv(&["git", "clone", "https://example.com/org/test-repo.git", "."]);
    assert!(
        backend.clone_target(&args, None).is_none(),
        "clone into '.' with no cwd_hint should return None — \
             relative '.' cannot be reliably resolved"
    );
}

#[test]
fn init_target_resolves_dot_when_cwd_hint_is_provided() {
    let backend = SystemGitBackend::new();
    // Use temp_dir() so the base path is absolute on all platforms (Windows
    // does not consider Unix-style paths like "/home/..." absolute).
    let cwd = std::env::temp_dir().join("git-ai-test-my-repo");
    assert!(
        cwd.is_absolute(),
        "temp_dir should be absolute on all platforms"
    );
    let args = argv(&["git", "init"]);
    let result = backend.init_target(&args, Some(&cwd)).unwrap();
    assert!(
        result.is_absolute(),
        "result must be absolute when cwd_hint is provided"
    );
    assert!(
        result.starts_with(&cwd),
        "result should be rooted at the cwd"
    );
}

// --- Bug (pre-existing): `--dissociate` is a boolean flag but was listed in
// `takes_value`, causing the next argument (typically the URL) to be swallowed
// as its "value".  `git clone --reference /mirror --dissociate <url>` would leave
// the positionals list empty and `clone_target()` would return None.

#[test]
fn clone_positionals_treats_dissociate_as_boolean_not_value_taking() {
    let args = argv(&[
        "--reference",
        "/mirror",
        "--dissociate",
        "https://example.com/org/test-repo.git",
    ]);
    assert_eq!(
        clone_init_positionals(&args),
        vec!["https://example.com/org/test-repo.git".to_string()],
        "--dissociate is boolean and must not consume the following URL"
    );
}

#[test]
fn builtin_primary_command_skips_repository_lookup() {
    let backend = SystemGitBackend::new();
    let missing_worktree = PathBuf::from("/definitely/missing/git-ai-backend-test");
    let argv = vec!["git".to_string(), "commit".to_string()];

    let resolved = backend
        .resolve_primary_command(&missing_worktree, &argv)
        .expect("builtin commands should not require repository discovery");

    assert_eq!(resolved.as_deref(), Some("commit"));
}

#[test]
fn resolve_family_uses_worktree_filesystem_without_git_config() {
    let temp = tempfile::tempdir().expect("tempdir");
    let git_dir = temp.path().join(".git");
    fs::create_dir_all(&git_dir).expect("create git dir");
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").expect("write HEAD");

    let family = SystemGitBackend::new()
        .resolve_family(temp.path())
        .expect("resolve family");

    assert_eq!(
        family.0,
        git_dir
            .canonicalize()
            .expect("canonical git dir")
            .to_string_lossy()
    );
}

#[test]
fn resolve_family_accepts_bare_repo_path_without_git_spawn() {
    let bare = tempfile::tempdir().expect("bare tempdir");
    fs::write(bare.path().join("HEAD"), "ref: refs/heads/main\n").expect("write HEAD");

    let family = SystemGitBackend::new()
        .resolve_family(bare.path())
        .expect("resolve family");

    assert_eq!(
        family.0,
        bare.path()
            .canonicalize()
            .expect("canonical bare dir")
            .to_string_lossy()
    );
}

#[test]
fn default_clone_target_from_url() {
    assert_eq!(
        default_clone_target_from_source("https://github.com/user/repo.git"),
        Some(PathBuf::from("repo"))
    );
    assert_eq!(
        default_clone_target_from_source("git@github.com:user/repo.git"),
        Some(PathBuf::from("repo"))
    );
    assert_eq!(
        default_clone_target_from_source("/local/path/repo"),
        Some(PathBuf::from("repo"))
    );
}

#[test]
fn default_clone_target_from_windows_path() {
    assert_eq!(
        default_clone_target_from_source(r"C:\Users\runner\Temp\repo"),
        Some(PathBuf::from("repo"))
    );
    assert_eq!(
        default_clone_target_from_source(r"C:\Users\runner\Temp\repo.git"),
        Some(PathBuf::from("repo"))
    );
    assert_eq!(
        default_clone_target_from_source(r"\\?\C:\Temp\bare-repo"),
        Some(PathBuf::from("bare-repo"))
    );
}

#[test]
fn unknown_primary_command_still_requires_repository_lookup() {
    let backend = SystemGitBackend::new();
    let missing_worktree = PathBuf::from("/definitely/missing/git-ai-backend-test");
    let argv = vec!["git".to_string(), "ci".to_string()];

    assert!(
        backend
            .resolve_primary_command(&missing_worktree, &argv)
            .is_err(),
        "unknown commands should still consult repository alias config"
    );
}
