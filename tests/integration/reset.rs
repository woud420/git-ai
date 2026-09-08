use crate::repos::test_file::{AuthorType, ExpectedLineExt};
use crate::repos::test_repo::TestRepo;
use std::fs;

fn assert_forward_reset_preserves_uncommitted_ai_attribution(reset_mode: &str) {
    let repo = TestRepo::new();
    let feature_path = repo.path().join("feature.txt");
    let upstream_path = repo.path().join("upstream.txt");

    fs::write(&feature_path, "human line\n").unwrap();
    fs::write(&upstream_path, "upstream v1\n").unwrap();
    let base = repo.stage_all_and_commit("Base commit").unwrap();

    let mut feature = repo.filename("feature.txt");
    feature.assert_committed_lines(crate::lines!["human line".unattributed_human()]);
    let mut upstream_file = repo.filename("upstream.txt");
    upstream_file.assert_committed_lines(crate::lines!["upstream v1".unattributed_human()]);

    fs::write(&upstream_path, "upstream v2\n").unwrap();
    let upstream = repo.stage_all_and_commit("Upstream commit").unwrap();

    feature.assert_committed_lines(crate::lines!["human line".unattributed_human()]);
    upstream_file.assert_committed_lines(crate::lines!["upstream v2".unattributed_human()]);

    repo.git(&["reset", "--hard", &base.commit_sha]).unwrap();
    feature.assert_committed_lines(crate::lines!["human line".unattributed_human()]);
    upstream_file.assert_committed_lines(crate::lines!["upstream v1".unattributed_human()]);

    // Model an AI agent's before/after checkpoints while HEAD is on the old base.
    repo.git_ai(&["checkpoint", "human", "feature.txt"])
        .unwrap();
    fs::write(&feature_path, "human line\nAI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "feature.txt"])
        .unwrap();

    // The upstream commit changes another file, so --keep can safely advance HEAD.
    repo.git(&["reset", reset_mode, &upstream.commit_sha])
        .unwrap();

    repo.git(&["add", "feature.txt"]).unwrap();
    repo.commit(&format!("Commit AI work after forward reset {reset_mode}"))
        .unwrap();

    feature.assert_committed_lines(crate::lines![
        "human line".unattributed_human(),
        "AI line".ai(),
    ]);
}

mod pathspecs;
mod recommit_attribution;
mod reset_modes;
