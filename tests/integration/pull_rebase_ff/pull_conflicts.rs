use super::{setup_conflict_pull_test, setup_pull_rebase_skip_test};

#[test]
fn test_pull_rebase_skip_commit_does_not_map_entire_upstream_history() {
    let (local, _upstream, local_ai_sha) = setup_pull_rebase_skip_test();

    local
        .git(&["pull", "--rebase"])
        .expect("pull --rebase should succeed");

    // HEAD should move away from original local commit onto upstream tip.
    let new_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();
    assert_ne!(
        new_head, local_ai_sha,
        "HEAD should have moved to upstream history after skipped rebase"
    );

    // Local commit was duplicated upstream via equivalent patch, so rebase should skip it.
    // Verify via git notes that the upstream-only commits (upstream_extra_1, upstream_extra_2)
    // did NOT receive AI authorship notes from the skipped local commit.
    // Walk backwards from HEAD: HEAD = upstream_extra_2, HEAD~1 = upstream_extra_1
    let upstream_extra_2 = new_head.clone();
    let upstream_extra_1 = local
        .git(&["rev-parse", "HEAD~1"])
        .expect("rev-parse HEAD~1")
        .trim()
        .to_string();

    assert!(
        local.read_authorship_note(&upstream_extra_2).is_none()
            || !local
                .read_authorship_note(&upstream_extra_2)
                .unwrap()
                .contains("ai_feature.txt"),
        "upstream_extra_2 should not have AI authorship notes from the skipped local commit"
    );
    assert!(
        local.read_authorship_note(&upstream_extra_1).is_none()
            || !local
                .read_authorship_note(&upstream_extra_1)
                .unwrap()
                .contains("ai_feature.txt"),
        "upstream_extra_1 should not have AI authorship notes from the skipped local commit"
    );
}

#[test]
fn test_pull_rebase_with_conflict_preserves_ai_notes() {
    let setup = setup_conflict_pull_test();
    let local = setup.local;

    // Verify Session B's AI commit has authorship notes before rebase
    let pre_rebase_note = local.read_authorship_note(&setup.session_b_ai_commit_sha);
    assert!(
        pre_rebase_note.is_some(),
        "Session B's AI commit should have authorship notes before rebase"
    );

    // Configure pull to use rebase (matching the doc scenario)
    local
        .git(&["config", "pull.rebase", "true"])
        .expect("set pull.rebase should succeed");

    // Fetch so we know about upstream's diverged state
    local
        .git(&["fetch", "origin"])
        .expect("fetch should succeed");

    // Pull will rebase — this should conflict on README.md
    let pull_result = local.git(&["pull"]);
    assert!(
        pull_result.is_err(),
        "pull --rebase should fail due to conflict on README.md"
    );

    // Resolve the conflict: keep both sessions' contributions
    use std::fs;
    fs::write(
        local.path().join("README.md"),
        "# Project\nSession A: AI-enhanced line 1\nSession A: AI-enhanced line 2\nSession B: AI-generated line 1\nSession B: AI-generated line 2\n",
    )
    .expect("writing resolved file should succeed");

    local
        .git(&["add", "README.md"])
        .expect("staging resolved file should succeed");

    local
        .git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should succeed");

    // The rebased commit has a new SHA
    let new_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    assert_ne!(
        new_head, setup.session_b_ai_commit_sha,
        "HEAD should have a new SHA after rebase"
    );

    // This is the core assertion from the doc:
    // After rebase, the new commit SHA should still have authorship notes.
    let post_rebase_note = local.read_authorship_note(&new_head);
    assert!(
        post_rebase_note.is_some(),
        "Rebased commit should have authorship notes (notes should follow SHA rewrite)"
    );

    // Verify the note content references the AI-authored file
    let note_content = post_rebase_note.unwrap();
    assert!(
        note_content.contains("README.md"),
        "Authorship note should reference README.md, got: {}",
        note_content
    );
}

// =============================================================================
// Pull --rebase abort preserves original notes
// =============================================================================

#[test]
fn test_pull_rebase_with_conflict_abort_preserves_original_notes() {
    let setup = setup_conflict_pull_test();
    let local = setup.local;

    // Verify Session B's AI commit has authorship notes before rebase
    let pre_rebase_note = local.read_authorship_note(&setup.session_b_ai_commit_sha);
    assert!(
        pre_rebase_note.is_some(),
        "Session B's AI commit should have authorship notes before rebase"
    );

    // Configure pull to use rebase
    local
        .git(&["config", "pull.rebase", "true"])
        .expect("set pull.rebase should succeed");

    // Fetch so we know about upstream's diverged state
    local
        .git(&["fetch", "origin"])
        .expect("fetch should succeed");

    // Pull will rebase — this should conflict on README.md
    let pull_result = local.git(&["pull"]);
    assert!(
        pull_result.is_err(),
        "pull --rebase should fail due to conflict on README.md"
    );

    // Abort the rebase instead of resolving
    local
        .git(&["rebase", "--abort"])
        .expect("rebase --abort should succeed");

    // Verify HEAD is back to Session B's original SHA
    let current_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    assert_eq!(
        current_head, setup.session_b_ai_commit_sha,
        "HEAD should be back to Session B's original commit after abort"
    );

    // Verify authorship notes on the original SHA are still intact
    let post_abort_note = local.read_authorship_note(&setup.session_b_ai_commit_sha);
    assert!(
        post_abort_note.is_some(),
        "Session B's AI commit should still have authorship notes after abort"
    );
}

crate::reuse_tests_in_worktree!(
    test_pull_rebase_skip_commit_does_not_map_entire_upstream_history,
    test_pull_rebase_with_conflict_preserves_ai_notes,
    test_pull_rebase_with_conflict_abort_preserves_original_notes,
);
