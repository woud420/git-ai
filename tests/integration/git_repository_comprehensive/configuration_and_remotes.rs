use super::{ExpectedLineExt, TestRepo, find_repository};

// ============================================================================
// Config Operations Tests
// ============================================================================

#[test]
fn test_config_get_str() {
    let test_repo = TestRepo::new();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Get user.name which is set in test repo
    let name = repo.config_get_str("user.name");
    assert!(name.is_ok(), "Should get config value");

    let name = name.unwrap();
    assert!(name.is_some(), "user.name should be set");
    assert_eq!(
        name.unwrap(),
        "Test User",
        "user.name should be 'Test User'"
    );
}

#[test]
fn test_config_get_str_nonexistent() {
    let test_repo = TestRepo::new();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Get nonexistent config
    let result = repo.config_get_str("nonexistent.config.key");
    assert!(result.is_ok(), "Should not error on nonexistent key");

    let value = result.unwrap();
    assert!(value.is_none(), "Nonexistent key should return None");
}

#[test]
fn test_config_get_regexp() {
    let test_repo = TestRepo::new();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Get all user.* configs
    let configs = repo.config_get_regexp("user\\..*");
    assert!(configs.is_ok(), "Should get matching configs");

    let configs = configs.unwrap();
    assert!(
        !configs.is_empty(),
        "Should have at least one user.* config"
    );
    assert!(
        configs.contains_key("user.name"),
        "Should contain user.name"
    );
}

#[test]
fn test_git_version() {
    let test_repo = TestRepo::new();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let version = repo.git_version();
    assert!(version.is_some(), "Should get git version");

    let (major, _minor, _patch) = version.unwrap();
    assert!(major >= 2, "Git major version should be at least 2");
}

#[test]
fn test_git_supports_ignore_revs_file() {
    let test_repo = TestRepo::new();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Most modern git versions support this (added in 2.23.0)
    let supports = repo.git_supports_ignore_revs_file();
    let expected = if let Some((major, minor, _)) = repo.git_version() {
        major > 2 || (major == 2 && minor >= 23)
    } else {
        true
    };
    assert_eq!(
        supports, expected,
        "ignore-revs-file support should match git version threshold"
    );
}

// ============================================================================
// Remote Operations Tests
// ============================================================================

#[test]
fn test_remotes_empty() {
    let test_repo = TestRepo::new();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let remotes = repo.remotes().unwrap();
    assert!(
        remotes.is_empty() || remotes == vec!["".to_string()],
        "New repo should have no remotes"
    );
}

#[test]
fn test_remotes_with_origin() {
    let (mirror, _upstream) = TestRepo::new_with_remote();

    let repo = find_repository(&[
        "-C".to_string(),
        mirror.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let remotes = repo.remotes().unwrap();
    assert!(
        remotes.contains(&"origin".to_string()),
        "Cloned repo should have origin remote"
    );
}

#[test]
fn test_remotes_with_urls() {
    let (mirror, _upstream) = TestRepo::new_with_remote();

    let repo = find_repository(&[
        "-C".to_string(),
        mirror.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let remotes_with_urls = repo.remotes_with_urls().unwrap();
    assert!(
        !remotes_with_urls.is_empty(),
        "Should have remotes with URLs"
    );

    let has_origin = remotes_with_urls
        .iter()
        .any(|(name, _url)| name == "origin");
    assert!(has_origin, "Should have origin remote with URL");
}

#[test]
fn test_get_default_remote() {
    let (mirror, _upstream) = TestRepo::new_with_remote();

    let repo = find_repository(&[
        "-C".to_string(),
        mirror.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let default_remote = repo.get_default_remote().unwrap();
    assert!(default_remote.is_some(), "Should have default remote");
    assert_eq!(
        default_remote.unwrap(),
        "origin",
        "Default remote should be origin"
    );
}

#[test]
fn test_get_default_remote_no_remotes() {
    let test_repo = TestRepo::new();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    let default_remote = repo.get_default_remote().unwrap();
    // New repos might have an empty string as a remote or None
    assert!(
        default_remote.is_none() || default_remote == Some("".to_string()),
        "Repo without remotes should have no default or empty default"
    );
}

#[test]
fn test_resolve_author_spec() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.stage_all_and_commit("Test").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Resolve author by name
    let result = repo.resolve_author_spec("Test User");
    assert!(result.is_ok(), "Should resolve author spec");

    let author = result.unwrap();
    assert!(author.is_some(), "Should find author");
}

#[test]
fn test_resolve_author_spec_not_found() {
    let test_repo = TestRepo::new();

    // Create commit
    let mut file = test_repo.filename("test.txt");
    file.set_contents(crate::lines!["content".human()]);
    test_repo.stage_all_and_commit("Test").unwrap();

    let repo = find_repository(&[
        "-C".to_string(),
        test_repo.path().to_str().unwrap().to_string(),
    ])
    .unwrap();

    // Resolve nonexistent author
    let result = repo.resolve_author_spec("Nonexistent Author");
    assert!(result.is_ok(), "Should not error on nonexistent author");

    let author = result.unwrap();
    assert!(author.is_none(), "Should not find nonexistent author");
}

crate::reuse_tests_in_worktree!(
    test_config_get_str,
    test_config_get_str_nonexistent,
    test_config_get_regexp,
    test_git_version,
    test_git_supports_ignore_revs_file,
    test_remotes_empty,
    test_remotes_with_origin,
    test_remotes_with_urls,
    test_get_default_remote,
    test_get_default_remote_no_remotes,
    test_resolve_author_spec,
    test_resolve_author_spec_not_found,
);
