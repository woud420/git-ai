use crate::repos::test_repo::{TestRepo, real_git_executable};
use std::path::Path;
use std::process::Command;

fn run_real_git(args: &[&str]) -> String {
    let output = Command::new(real_git_executable())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run git {:?}: {}", args, e));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "git {:?} failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        args,
        output.status.code(),
        stdout,
        stderr
    );
    stdout.trim().to_string()
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("test path must be valid UTF-8")
}

mod command_contracts;
mod github_synchronization;
