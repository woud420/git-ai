#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::{TestRepo, default_branchname};
use std::fs;

#[path = "workspace_modes/branch.rs"]
mod branch;
#[path = "workspace_modes/checkout.rs"]
mod checkout;
#[path = "workspace_modes/index.rs"]
mod index;

fn repo_with_pending_ai() -> TestRepo {
    repo_with_pending_ai_in(TestRepo::new())
}

fn repo_with_pending_ai_in(repo: TestRepo) -> TestRepo {
    fs::write(repo.path().join("seed.txt"), "seed\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();
    repo.filename("seed.txt")
        .assert_committed_lines(lines!["seed".unattributed_human()]);
    write_ai(&repo, "pending.txt", "pending AI\n");
    repo
}

fn write_ai(repo: &TestRepo, file: &str, contents: &str) {
    repo.git_ai(&["checkpoint", "human", file]).unwrap();
    fs::write(repo.path().join(file), contents).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", file]).unwrap();
}

fn commit_and_assert_pending(repo: &TestRepo, message: &str) {
    repo.stage_all_and_commit(message).unwrap();
    repo.filename("seed.txt")
        .assert_committed_lines(lines!["seed".unattributed_human()]);
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["pending AI".ai()]);
}

fn run_workspace_command(repo: &TestRepo, args: &[&str]) {
    let before = repo.daemon_total_completion_count();
    repo.git_without_test_sync_for_test(args, &[]).unwrap();
    repo.wait_for_daemon_total_completion_count(before, before + 1);
}

fn wait_for_side_effect_gate(gate: &std::path::Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "command did not reach side-effect gate"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
