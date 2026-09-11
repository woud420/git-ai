use super::*;

#[test]
fn test_regular_rebase_conflict_ai_resolution_preserves_original_and_resolution_sessions() {
    use std::fs;

    let setup = setup_regular_rebase_conflict();
    let repo = setup.repo;
    let original_log = repo.require_authorship_log(&setup.feature_ai_commit_sha);
    let original_sessions = session_keys(&original_log);
    assert!(
        !original_sessions.is_empty(),
        "precondition: original feature note should contain session metadata"
    );

    let rebase_result = repo.git(&["rebase", &setup.default_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should fail due to conflict on shared.txt"
    );

    repo.git_ai(&["checkpoint", "human", "shared.txt"])
        .expect("pre-resolution checkpoint should succeed");
    fs::write(
        repo.path().join("shared.txt"),
        "line 1\nmain change line 2\nAI resolved line 2",
    )
    .expect("write AI conflict resolution");
    repo.git_ai(&["checkpoint", "mock_ai", "shared.txt"])
        .expect("AI resolution checkpoint should succeed");

    repo.git(&["add", "shared.txt"])
        .expect("staging resolved file should succeed");
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should succeed");

    let rebased_head = repo
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();
    assert_ne!(
        rebased_head, setup.feature_ai_commit_sha,
        "HEAD should have a new SHA after rebase"
    );

    let rebased_note = repo
        .read_authorship_note(&rebased_head)
        .expect("rebased commit should have authorship note");
    let rebased_log =
        AuthorshipLog::deserialize_from_string(&rebased_note).expect("parse rebased note");
    let rebased_sessions = session_keys(&rebased_log);
    let resolution_sessions = rebased_sessions
        .difference(&original_sessions)
        .cloned()
        .collect::<BTreeSet<_>>();

    assert!(
        original_sessions.is_subset(&rebased_sessions),
        "rebased note should preserve original feature session metadata; original={:?}, rebased={:?}; note={}",
        original_sessions,
        rebased_sessions,
        rebased_note
    );
    assert!(
        !resolution_sessions.is_empty(),
        "rebased note should contain a new AI conflict-resolution session; original={:?}, rebased={:?}",
        original_sessions,
        rebased_sessions
    );

    let shared_authors = attestation_author_keys(&rebased_log, "shared.txt");
    assert!(
        !shared_authors.is_empty(),
        "AI resolution should create shared.txt attribution"
    );
    assert!(
        shared_authors
            .iter()
            .any(|author| resolution_sessions.contains(author)),
        "shared.txt attribution should belong to resolution session; authors={:?}, resolution_sessions={:?}",
        shared_authors,
        resolution_sessions
    );
    assert!(
        shared_authors.is_disjoint(&original_sessions),
        "original conflict-hunk attribution should be dropped, not carried as file attribution; authors={:?}, original_sessions={:?}",
        shared_authors,
        original_sessions
    );

    let mut final_file = repo.filename("shared.txt");
    final_file.assert_committed_lines(crate::lines![
        "line 1".human(),
        "main change line 2".human(),
        "AI resolved line 2".ai(),
    ]);
}

#[test]
fn test_regular_rebase_conflict_keep_feature_side_preserves_feature_attribution() {
    use std::fs;

    let setup = setup_regular_rebase_conflict();
    let repo = setup.repo;
    let original_log = repo.require_authorship_log(&setup.feature_ai_commit_sha);
    let original_sessions = session_keys(&original_log);
    assert!(
        !original_sessions.is_empty(),
        "precondition: original feature note should contain session metadata"
    );

    let rebase_result = repo.git(&["rebase", &setup.default_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should fail due to conflict on shared.txt"
    );

    fs::write(repo.path().join("shared.txt"), "line 1\nAI feature line 2")
        .expect("write feature-side conflict resolution");
    repo.git(&["add", "shared.txt"])
        .expect("staging resolved file should succeed");
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should succeed");

    let rebased_head = repo
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();
    let rebased_note = repo
        .read_authorship_note(&rebased_head)
        .expect("rebased commit should have authorship note");
    let rebased_log =
        AuthorshipLog::deserialize_from_string(&rebased_note).expect("parse rebased note");
    let shared_authors = attestation_author_keys(&rebased_log, "shared.txt");
    assert!(
        shared_authors
            .iter()
            .any(|author| original_sessions.contains(author)),
        "feature-side resolution should preserve feature attribution; authors={:?}, original_sessions={:?}; note={}",
        shared_authors,
        original_sessions,
        rebased_note
    );

    let mut final_file = repo.filename("shared.txt");
    final_file.assert_committed_lines(crate::lines!["line 1".human(), "AI feature line 2".ai(),]);
}

#[test]
fn test_regular_rebase_conflict_keep_both_sides_preserves_each_original_source() {
    use std::fs;

    let setup = setup_regular_rebase_conflict_with_trailing_newlines();
    let repo = setup.repo;
    let original_log = repo.require_authorship_log(&setup.feature_ai_commit_sha);
    let original_sessions = session_keys(&original_log);
    assert!(
        !original_sessions.is_empty(),
        "precondition: original feature note should contain session metadata"
    );

    let rebase_result = repo.git(&["rebase", &setup.default_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should fail due to conflict on shared.txt"
    );

    fs::write(
        repo.path().join("shared.txt"),
        "line 1\nmain change line 2\nAI feature line 2\n",
    )
    .expect("write keep-both conflict resolution");
    repo.git(&["add", "shared.txt"])
        .expect("staging resolved file should succeed");
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should succeed");

    let rebased_head = repo
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();
    let rebased_note = repo
        .read_authorship_note(&rebased_head)
        .expect("rebased commit should have authorship note");
    let rebased_log =
        AuthorshipLog::deserialize_from_string(&rebased_note).expect("parse rebased note");
    let shared_authors = attestation_author_keys(&rebased_log, "shared.txt");
    assert!(
        shared_authors
            .iter()
            .any(|author| original_sessions.contains(author)),
        "keep-both resolution should preserve feature-side attribution; authors={:?}, original_sessions={:?}; note={}",
        shared_authors,
        original_sessions,
        rebased_note
    );

    let blame = repo
        .git(&["blame", "--line-porcelain", "-L", "2,2", "--", "shared.txt"])
        .expect("git blame should succeed");
    let blamed_commit = blame
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .expect("blame should include commit sha");
    assert_eq!(
        blamed_commit, setup.main_conflict_commit_sha,
        "main-side kept line should blame to the original main conflict commit"
    );

    let mut final_file = repo.filename("shared.txt");
    final_file.assert_committed_lines(crate::lines![
        "line 1".human(),
        "main change line 2".human(),
        "AI feature line 2".ai(),
    ]);
}

#[test]
fn test_regular_rebase_conflict_keep_main_side_preserves_main_attribution() {
    use std::fs;

    let setup = setup_regular_rebase_conflict();
    let repo = setup.repo;

    let rebase_result = repo.git(&["rebase", &setup.default_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should fail due to conflict on shared.txt"
    );

    fs::write(repo.path().join("shared.txt"), "line 1\nmain change line 2")
        .expect("write main-side conflict resolution");
    repo.git(&["add", "shared.txt"])
        .expect("staging resolved file should succeed");
    repo.git(&["rebase", "--skip"])
        .expect("main-side resolution makes the feature commit empty, so rebase should skip it");

    let head = repo
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();
    assert_eq!(
        head, setup.main_conflict_commit_sha,
        "keeping the main side should leave feature at the original main conflict commit"
    );

    let blame = repo
        .git(&["blame", "--line-porcelain", "-L", "2,2", "--", "shared.txt"])
        .expect("git blame should succeed");
    let blamed_commit = blame
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .expect("blame should include commit sha");
    assert_eq!(
        blamed_commit, setup.main_conflict_commit_sha,
        "main-side line should blame to the original main conflict commit"
    );

    let mut final_file = repo.filename("shared.txt");
    final_file.assert_committed_lines(crate::lines![
        "line 1".human(),
        "main change line 2".human(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_regular_rebase_conflict_ai_resolution_preserves_original_and_resolution_sessions,
    test_regular_rebase_conflict_keep_feature_side_preserves_feature_attribution,
    test_regular_rebase_conflict_keep_both_sides_preserves_each_original_source,
    test_regular_rebase_conflict_keep_main_side_preserves_main_attribution,
);
