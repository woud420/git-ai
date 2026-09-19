use super::*;

#[test]
fn committing_earlier_ai_checkpoint_preserves_later_replacement_carryover() {
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
    repo.stage_all_and_commit("commit later AI checkpoint")
        .unwrap();
    repo.filename("test.txt")
        .assert_committed_lines(crate::lines!["later AI version".ai()]);
}

#[test]
fn checkpointed_human_replacement_survives_an_earlier_ai_commit() {
    let repo = seeded_repo();
    let path = repo.path().join("test.txt");
    fs::write(&path, "earlier AI version\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    fs::write(&path, "later human version\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();
    repo.commit("commit earlier AI checkpoint").unwrap();
    assert_head_lines(&repo, crate::lines!["earlier AI version".ai()]);
    repo.stage_all_and_commit("commit later known-human checkpoint")
        .unwrap();
    repo.filename("test.txt")
        .assert_committed_lines(crate::lines!["later human version".human()]);
    let head = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    let log = repo.require_authorship_log(head.trim());
    assert!(
        log.metadata.humans.contains_key(&owner_at_head(&repo, 1)),
        "known-human carryover must retain explicit evidence, not only non-AI blame"
    );
}

#[test]
fn partial_commit_preserves_replacement_session_and_shifted_pending_lines() {
    let repo = seeded_repo();
    let path = repo.path().join("test.txt");
    fs::write(&path, "earlier AI version\nanchor\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    fs::write(&path, "later AI version\nextra AI line\nanchor\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.commit("commit first session").unwrap();
    assert_head_lines(
        &repo,
        crate::lines!["earlier AI version".ai(), "anchor".ai()],
    );
    let first = owner_at_head(&repo, 1);
    repo.stage_all_and_commit("commit second session replacement and insertion")
        .unwrap();
    repo.filename("test.txt")
        .assert_committed_lines(crate::lines![
            "later AI version".ai(),
            "extra AI line".ai(),
            "anchor".ai(),
        ]);
    let second = owner_at_head(&repo, 1);
    assert_ne!(
        first, second,
        "the later replacement must retain its original session"
    );
    assert_eq!(second, owner_at_head(&repo, 2));
}

#[test]
fn an_uncheckpointed_replacement_after_partial_commit_does_not_inherit_pending_ai() {
    let repo = seeded_repo();
    let path = repo.path().join("test.txt");
    fs::write(&path, "first AI version\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.git(&["add", "test.txt"]).unwrap();
    fs::write(&path, "later AI version\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    repo.commit("commit earlier checkpoint").unwrap();
    assert_head_lines(&repo, crate::lines!["first AI version".ai()]);
    fs::write(&path, "unknown version!\n").unwrap();
    repo.stage_all_and_commit("replace pending AI without evidence")
        .unwrap();
    repo.filename("test.txt")
        .assert_committed_lines(crate::lines!["unknown version!".unattributed_human()]);
    assert_untracked_line(&repo, 1);
}

fn owner_at_head(repo: &TestRepo, line: u32) -> String {
    let head = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    let log = repo.require_authorship_log(head.trim());
    log.attestations
        .iter()
        .filter(|file| file.file_path == "test.txt")
        .flat_map(|file| &file.entries)
        .find(|entry| entry.line_ranges.iter().any(|range| range.contains(line)))
        .expect("the committed line has an explicit author")
        .hash
        .clone()
}

#[test]
fn an_unequal_uncheckpointed_replacement_remains_untracked_beside_retained_ai() {
    let repo = seeded_repo();
    let path = repo.path().join("test.txt");
    fs::write(&path, "AI line\nretained AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();
    fs::write(&path, "untracked one\nuntracked two\nretained AI\n").unwrap();
    repo.stage_all_and_commit("replace one AI line with two untracked lines")
        .unwrap();
    repo.filename("test.txt")
        .assert_committed_lines(crate::lines![
            "untracked one".unattributed_human(),
            "untracked two".unattributed_human(),
            "retained AI".ai(),
        ]);
    assert_untracked_line(&repo, 1);
    assert_untracked_line(&repo, 2);
}
