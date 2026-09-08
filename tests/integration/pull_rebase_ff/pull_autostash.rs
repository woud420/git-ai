use super::{ExpectedLineExt, setup_divergent_pull_test};

// =============================================================================
// Pull --rebase --autostash with uncommitted changes
// =============================================================================

#[test]
fn test_pull_rebase_autostash_preserves_uncommitted_ai_attribution() {
    let setup = setup_divergent_pull_test();
    let local = setup.local;

    // Add uncommitted AI changes on top of the committed ones
    let mut uncommitted_ai = local.filename("uncommitted_ai.txt");
    uncommitted_ai.set_contents(vec![
        "AI generated line 1".ai(),
        "AI generated line 2".ai(),
        "AI generated line 3".ai(),
    ]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Pull --rebase --autostash: uncommitted changes get stashed/unstashed
    local
        .git(&["pull", "--rebase", "--autostash"])
        .expect("pull --rebase --autostash should succeed");

    // Commit the previously-uncommitted changes
    local
        .stage_all_and_commit("commit after rebase pull")
        .expect("commit should succeed");

    uncommitted_ai.assert_lines_and_blame(vec![
        "AI generated line 1".ai(),
        "AI generated line 2".ai(),
        "AI generated line 3".ai(),
    ]);
}

#[test]
fn test_pull_rebase_autostash_with_mixed_attribution() {
    let setup = setup_divergent_pull_test();
    let local = setup.local;

    // Create local uncommitted changes with mixed human and AI attribution
    let mut mixed_file = local.filename("mixed_work.txt");
    mixed_file.set_contents(vec![
        "Human written line 1".human(),
        "AI generated line 1".ai(),
        "Human written line 2".human(),
        "AI generated line 2".ai(),
    ]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Pull --rebase --autostash
    local
        .git(&["pull", "--rebase", "--autostash"])
        .expect("pull --rebase --autostash should succeed");

    // Commit and verify mixed attribution is preserved
    local
        .stage_all_and_commit("commit with mixed attribution")
        .expect("commit should succeed");

    mixed_file.assert_lines_and_blame(vec![
        "Human written line 1".human(),
        "AI generated line 1".ai(),
        "Human written line 2".human(),
        "AI generated line 2".ai(),
    ]);
}

// =============================================================================
// Pull --rebase with both committed AND uncommitted changes
// =============================================================================

#[test]
fn test_pull_rebase_committed_and_autostash_preserves_all_authorship() {
    let setup = setup_divergent_pull_test();
    let local = setup.local;

    // Add uncommitted AI changes on top of the committed AI commit
    let mut uncommitted_ai = local.filename("uncommitted_ai.txt");
    uncommitted_ai.set_contents(vec!["Uncommitted AI line".ai()]);
    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Pull --rebase --autostash: committed changes get rebased, uncommitted get stashed
    local
        .git(&["pull", "--rebase", "--autostash"])
        .expect("pull --rebase --autostash should succeed");

    // Commit the previously-uncommitted changes
    local
        .stage_all_and_commit("commit uncommitted AI work")
        .expect("commit should succeed");

    // Verify committed AI authorship survived the rebase
    let mut committed_ai = local.filename("ai_feature.txt");
    committed_ai.assert_lines_and_blame(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);

    // Verify uncommitted AI authorship survived the autostash cycle
    uncommitted_ai.assert_lines_and_blame(vec!["Uncommitted AI line".ai()]);
}

crate::reuse_tests_in_worktree!(
    test_pull_rebase_autostash_preserves_uncommitted_ai_attribution,
    test_pull_rebase_autostash_with_mixed_attribution,
    test_pull_rebase_committed_and_autostash_preserves_all_authorship,
);
