use super::{ExpectedLineExt, TestRepo, fs};
use crate::repos::test_file::ExpectedLine;

fn assert_head_lines(repo: &TestRepo, expected: Vec<ExpectedLine>) {
    repo.sync_daemon();
    let path = repo.path().join("test.txt");
    let pending = fs::read(&path).unwrap();
    let committed = repo.git_og(&["show", "HEAD:test.txt"]).unwrap();
    // The blame helper inspects the worktree. Restore HEAD only after the daemon
    // has finalized the note, then put the pending version back for the next commit.
    fs::write(&path, committed).unwrap();
    repo.filename("test.txt").assert_committed_lines(expected);
    fs::write(path, pending).unwrap();
}

fn assert_untracked_line(repo: &TestRepo, line: u32) {
    let head = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    if let Some(note) = repo.read_authorship_note(head.trim()) {
        let log =
            git_ai::model::authorship_log_serialization::AuthorshipLog::deserialize_from_string(
                &note,
            )
            .unwrap();
        assert!(
            log.attestations
                .iter()
                .filter(|file| file.file_path == "test.txt")
                .flat_map(|file| &file.entries)
                .all(|entry| entry.line_ranges.iter().all(|range| !range.contains(line))),
            "untracked replacement must have neither AI nor known-human attestation"
        );
    }
}

fn seeded_repo() -> TestRepo {
    let repo = TestRepo::new();
    fs::write(repo.path().join("test.txt"), "base\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();
    repo.filename("test.txt")
        .assert_committed_lines(crate::lines!["base".unattributed_human()]);
    repo
}

#[test]
fn overwritten_pending_ai_content_remains_untracked_beside_retained_ai() {
    let repo = seeded_repo();
    let path = repo.path().join("test.txt");
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(&path, "AAAAAAAAAA\nAI line that remains\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    fs::write(&path, "ZZZZZZZZZZ\nAI line that remains\n").unwrap();
    repo.stage_all_and_commit("replace pending AI content without evidence")
        .unwrap();
    repo.filename("test.txt")
        .assert_committed_lines(crate::lines![
            "ZZZZZZZZZZ".unattributed_human(),
            "AI line that remains".ai(),
        ]);
    assert_untracked_line(&repo, 1);
}

#[test]
fn unstaged_replacement_does_not_invalidate_committed_ai_evidence() {
    let repo = seeded_repo();
    let path = repo.path().join("test.txt");
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(&path, "AI staged version\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    fs::write(&path, "untracked worktree version\n").unwrap();

    repo.commit("commit staged AI content").unwrap();
    assert_head_lines(&repo, crate::lines!["AI staged version".ai()]);
    repo.stage_all_and_commit("commit later untracked replacement")
        .unwrap();
    repo.filename("test.txt")
        .assert_committed_lines(crate::lines![
            "untracked worktree version".unattributed_human(),
        ]);
    assert_untracked_line(&repo, 1);
}

#[test]
fn committing_earlier_ai_checkpoint_preserves_its_attribution() {
    let repo = seeded_repo();
    let path = repo.path().join("test.txt");
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(&path, "first AI version\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    fs::write(&path, "later AI version\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    repo.commit("commit earlier AI checkpoint").unwrap();
    assert_head_lines(&repo, crate::lines!["first AI version".ai()]);
}

#[test]
fn restored_known_human_checkpoint_retains_explicit_human_evidence() {
    let repo = seeded_repo();
    let path = repo.path().join("test.txt");
    fs::write(&path, "known human version\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    fs::write(&path, "AI version\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    fs::write(&path, "known human version\n").unwrap();
    repo.stage_all_and_commit("restore checkpointed human content")
        .unwrap();
    repo.filename("test.txt")
        .assert_committed_lines(crate::lines!["known human version".human()]);
    let head = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    let log = repo.require_authorship_log(head.trim());
    assert!(
        log.attestations
            .iter()
            .filter(|file| file.file_path == "test.txt")
            .flat_map(|file| &file.entries)
            .any(|entry| log.metadata.humans.contains_key(&entry.hash)
                && entry.line_ranges.iter().any(|range| range.contains(1))),
        "restored known-human content must retain explicit evidence"
    );
}

#[test]
fn staged_content_from_different_ai_checkpoints_retains_both_authors() {
    let repo = TestRepo::new();
    let path = repo.path().join("test.txt");
    fs::write(&path, "old top\nanchor\nold bottom\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();
    repo.filename("test.txt")
        .assert_committed_lines(crate::lines![
            "old top".unattributed_human(),
            "anchor".unattributed_human(),
            "old bottom".unattributed_human(),
        ]);
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(&path, "first AI top\nanchor\nold bottom\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    fs::write(&path, "later AI top\nanchor\nAI bottom\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    fs::write(&path, "first AI top\nanchor\nAI bottom\n").unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    fs::write(&path, "later AI top\nanchor\nAI bottom\n").unwrap();
    repo.commit("commit staged changes from both checkpoints")
        .unwrap();
    assert_head_lines(
        &repo,
        crate::lines![
            "first AI top".ai(),
            "anchor".unattributed_human(),
            "AI bottom".ai(),
        ],
    );
    let head = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    let log = repo.require_authorship_log(head.trim());
    let owner = |line| {
        log.attestations
            .iter()
            .filter(|file| file.file_path == "test.txt")
            .flat_map(|file| &file.entries)
            .find(|entry| entry.line_ranges.iter().any(|range| range.contains(line)))
            .unwrap()
            .hash
            .split("::")
            .next()
            .unwrap()
            .to_owned()
    };
    assert_ne!(
        owner(1),
        owner(3),
        "each checkpoint's original session must survive"
    );
}
