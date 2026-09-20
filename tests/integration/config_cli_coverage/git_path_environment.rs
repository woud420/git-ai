use super::{DaemonTestScope, TestRepo, get_json_with_env};
use crate::repos::test_repo::get_binary_path;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn configured_repo() -> (TestRepo, PathBuf) {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let real_git = get_json_with_env(&repo, "git_path", &[("GIT_AI_GIT_PATH", "")]);
    let configured = repo.test_home_path().join("configured-git.exe");
    fs::copy(real_git.as_str().unwrap(), &configured).unwrap();
    let config_path = repo.test_home_path().join(".git-ai/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["git_path"] = configured.to_str().unwrap().into();
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    (repo, configured)
}

#[test]
fn environment_git_path_overrides_config_without_rewriting_it() {
    let (repo, configured) = configured_repo();
    let supplied = repo.test_home_path().join("supplied-git.exe");
    fs::copy(&configured, &supplied).unwrap();
    let config_path = repo.test_home_path().join(".git-ai/config.json");
    let original = fs::read(&config_path).unwrap();
    let padded = format!("  {}  ", supplied.display());

    assert_eq!(
        get_json_with_env(&repo, "git_path", &[("GIT_AI_GIT_PATH", &padded)]),
        supplied.to_str().unwrap()
    );
    assert_eq!(fs::read(config_path).unwrap(), original);
}

#[test]
fn invalid_environment_git_path_falls_back_to_config() {
    let (repo, configured) = configured_repo();
    let missing = repo.test_home_path().join("missing-git.exe");
    let binary = get_binary_path();
    for supplied in [
        " ",
        missing.to_str().unwrap(),
        repo.test_home_path().to_str().unwrap(),
        binary.to_str().unwrap(),
    ] {
        assert_eq!(
            get_json_with_env(&repo, "git_path", &[("GIT_AI_GIT_PATH", supplied)]),
            configured.to_str().unwrap(),
            "invalid or recursive Git path {supplied:?} must be ignored"
        );
    }
}

#[test]
fn environment_git_path_works_without_creating_config() {
    let (repo, supplied) = configured_repo();
    let config_path = repo.test_home_path().join(".git-ai/config.json");
    fs::remove_file(&config_path).unwrap();
    let envs = [("GIT_AI_GIT_PATH", supplied.to_str().unwrap())];

    assert_eq!(
        get_json_with_env(&repo, "git_path", &envs),
        supplied.to_str().unwrap()
    );
    assert!(!config_path.exists());
}
