#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;

fn write_ai(repo: &TestRepo, path: &str, contents: &str) {
    repo.git_ai(&["checkpoint", "human", path]).unwrap();
    fs::write(repo.path().join(path), contents).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", path]).unwrap();
}

fn pending_checkpoint_survives_untracked_history(mode: &str) {
    let repo = TestRepo::new();
    fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    let base = repo.stage_all_and_commit("base").unwrap().commit_sha;
    repo.filename("base.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    fs::write(repo.path().join("base.txt"), "base\nordinary edit\n").unwrap();
    repo.stage_all_and_commit("untracked edit").unwrap();
    repo.filename("base.txt").assert_committed_lines(lines![
        "base".unattributed_human(),
        "ordinary edit".unattributed_human()
    ]);
    write_ai(&repo, "pending.txt", "pending AI\n");
    repo.git(&["reset", mode, &base]).unwrap();
    repo.stage_all_and_commit("commit preserved workspace")
        .unwrap();
    repo.filename("base.txt").assert_committed_lines(lines![
        "base".unattributed_human(),
        "ordinary edit".unattributed_human()
    ]);
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["pending AI".ai()]);
}

fn pending_initial_takes_precedence_over_reconstructed_history(mode: &str) {
    let repo = TestRepo::new();
    fs::write(repo.path().join("file.txt"), "base\n").unwrap();
    let base = repo.stage_all_and_commit("base").unwrap().commit_sha;
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    write_ai(&repo, "file.txt", "base\nhistorical AI\n");
    repo.stage_all_and_commit("historical AI").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".unattributed_human(), "historical AI".ai()]);
    fs::write(repo.path().join("file.txt"), "base\npending human\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "file.txt"])
        .unwrap();
    write_ai(&repo, "other.txt", "other AI\n");
    repo.git(&["add", "other.txt"]).unwrap();
    repo.commit("partial commit").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".unattributed_human()]);
    repo.filename("other.txt")
        .assert_committed_lines(lines!["other AI".ai()]);
    repo.git(&["reset", mode, &base]).unwrap();
    repo.stage_all_and_commit("commit preserved pending human")
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".unattributed_human(), "pending human".human()]);
    repo.filename("other.txt")
        .assert_committed_lines(lines!["other AI".ai()]);
}

#[test]
fn soft_reset_keeps_pending_ai_without_historical_notes() {
    pending_checkpoint_survives_untracked_history("--soft");
}
#[test]
fn mixed_reset_keeps_pending_ai_without_historical_notes() {
    pending_checkpoint_survives_untracked_history("--mixed");
}
#[test]
fn soft_reset_keeps_pending_human_over_historical_ai() {
    pending_initial_takes_precedence_over_reconstructed_history("--soft");
}
#[test]
fn mixed_reset_keeps_pending_human_over_historical_ai() {
    pending_initial_takes_precedence_over_reconstructed_history("--mixed");
}
