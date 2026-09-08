use super::{TempDir, TestRepo, fs, resolve_repo_url_from_path};

// === Test Group 1: resolve_repo_url_from_path utility ===

#[test]
fn test_resolve_repo_url_from_path_ssh_remote() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "git@github.com:org/project.git"])
        .unwrap();
    let result = resolve_repo_url_from_path(repo.path());
    assert_eq!(
        result,
        Some("https://github.com/org/project".to_string()),
        "SSH remote must be normalized to HTTPS"
    );
}

#[test]
fn test_resolve_repo_url_from_path_https_remote() {
    let repo = TestRepo::new();
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/org/project.git",
    ])
    .unwrap();
    let result = resolve_repo_url_from_path(repo.path());
    assert_eq!(
        result,
        Some("https://github.com/org/project".to_string()),
        "HTTPS remote must strip .git suffix"
    );
}

#[test]
fn test_resolve_repo_url_from_path_no_remote() {
    let repo = TestRepo::new();
    let result = resolve_repo_url_from_path(repo.path());
    assert_eq!(result, None, "Must return None when repo has no remote");
}

#[test]
fn test_resolve_repo_url_from_path_not_a_repo() {
    let temp_dir = TempDir::new().unwrap();
    let result = resolve_repo_url_from_path(temp_dir.path());
    assert_eq!(result, None, "Must return None for non-repo directory");
}

#[test]
fn test_resolve_repo_url_from_path_strips_credentials() {
    let repo = TestRepo::new();
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://user:token@github.com/org/project.git",
    ])
    .unwrap();
    let result = resolve_repo_url_from_path(repo.path());
    assert_eq!(
        result,
        Some("https://github.com/org/project".to_string()),
        "Credentials must be stripped from repo_url"
    );
}

#[test]
fn test_resolve_repo_url_from_path_from_subdirectory() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "git@github.com:org/project.git"])
        .unwrap();
    let subdir = repo.path().join("src/deep/nested");
    fs::create_dir_all(&subdir).unwrap();
    let result = resolve_repo_url_from_path(&subdir);
    assert_eq!(
        result,
        Some("https://github.com/org/project".to_string()),
        "Must resolve repo_url from subdirectory"
    );
}
