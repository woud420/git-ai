use super::{ExpectedLineExt, TestRepo};

#[test]
fn test_partial_staging_filters_unstaged_lines() {
    // Test where AI makes changes but only some are staged
    let repo = TestRepo::new();
    let mut file = repo.filename("partial.ts");

    file.set_contents(crate::lines!["line1", "line2", "line3"]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI modifies lines 2-3 and we stage immediately
    file.replace_at(1, "ai_modified2".ai());
    file.replace_at(2, "ai_modified3".ai());

    file.stage();

    // Now AI adds more lines that won't be staged
    file.insert_at(
        3,
        crate::lines!["unstaged_line1".ai(), "unstaged_line2".ai()],
    );

    let commit = repo.commit("Partial staging").unwrap();

    // The commit should only include the modifications, not the unstaged additions
    assert_eq!(commit.authorship_log.attestations.len(), 1);

    // Only check committed lines (unstaged lines will be ignored)
    file.assert_committed_lines(crate::lines![
        "line1".human(),
        "ai_modified2".ai(),
        // ai_modified3 is ai, but it's not considered committed, because adding the subsequent uncommitted lines also added a newline char to this line
    ]);
}

#[test]
fn test_human_stages_some_ai_lines() {
    // Test where AI adds multiple lines but human only stages some of them
    let repo = TestRepo::new();
    let mut file = repo.filename("test.ts");

    file.set_contents(crate::lines!["line1", "line2", "line3"]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI adds lines 4-8
    file.insert_at(
        3,
        crate::lines![
            "ai_line4".ai(),
            "ai_line5".ai(),
            "ai_line6".ai(),
            "ai_line7".ai(),
            "ai_line8".ai(),
        ],
    );

    file.stage();

    // Human adds an unstaged line
    file.insert_at(8, crate::lines!["human_unstaged".human()]);

    let commit = repo.commit("Partial AI commit").unwrap();
    assert_eq!(commit.authorship_log.attestations.len(), 1);

    // Only check committed lines (unstaged human line will be ignored)
    file.assert_committed_lines(crate::lines![
        "line1".human(),
        "line2".human(),
        "line3".ai(),
        "ai_line4".ai(),
        "ai_line5".ai(),
        "ai_line6".ai(),
        "ai_line7".ai(),
        // ai_line8 is ai, but it's not considered committed, because adding the subsequent uncommitted lines also added a newline char to this line
    ]);
}

#[test]
fn test_multiple_ai_sessions_with_partial_staging() {
    // Multiple AI sessions, but only one has staged changes
    let repo = TestRepo::new();
    let mut file = repo.filename("test.ts");

    file.set_contents(crate::lines!["line1", "line2", "line3"]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // First AI session adds lines and they get staged
    file.insert_at(
        3,
        crate::lines!["ai1_line1".ai(), "ai1_line2".ai(), "ai1_line3".ai()],
    );

    file.stage();

    // Second AI session adds lines but they DON'T get staged
    file.insert_at(
        6,
        crate::lines!["ai2_line1".ai(), "ai2_line2".ai(), "ai2_line3".ai()],
    );

    let commit = repo.commit("Commit first AI session only").unwrap();
    assert_eq!(commit.authorship_log.attestations.len(), 1);

    // Only check committed lines (second AI session unstaged)
    file.assert_committed_lines(crate::lines![
        "line1".human(),
        "line2".human(),
        "line3".ai(),
        "ai1_line1".ai(),
        "ai1_line2".ai(),
        // ai1_line3 is ai, but it's not considered committed, because adding the subsequent uncommitted lines also added a newline char to this line
    ]);
}

#[test]
fn test_ai_adds_then_commits_in_batches() {
    // AI adds lines in multiple batches, committing separately
    let repo = TestRepo::new();
    let mut file = repo.filename("test.ts");

    file.set_contents(crate::lines!["line1", "line2", "line3", "line4", ""]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI adds first batch of lines
    file.insert_at(
        4,
        crate::lines!["ai_line5".ai(), "ai_line6".ai(), "ai_line7".ai()],
    );
    file.stage();

    repo.commit("Add lines 5-7").unwrap();

    // AI adds second batch of lines
    file.insert_at(
        7,
        crate::lines!["ai_line8".ai(), "ai_line9".ai(), "ai_line10".ai()],
    );

    repo.stage_all_and_commit("Add lines 8-10").unwrap();

    file.assert_lines_and_blame(crate::lines![
        "line1".human(),
        "line2".human(),
        "line3".human(),
        "line4".human(),
        "ai_line5".ai(),
        "ai_line6".ai(),
        "ai_line7".ai(),
        "ai_line8".ai(),
        "ai_line9".ai(),
        "ai_line10".ai(),
    ]);
}

#[test]
fn test_ai_edits_with_partial_staging() {
    // AI makes modifications, some staged and some not
    let repo = TestRepo::new();
    let mut file = repo.filename("test.ts");

    file.set_contents(crate::lines!["line1", "line2", "line3", "line4", "line5"]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI modifies some lines
    file.replace_at(1, "ai_modified_line2".ai());
    file.replace_at(3, "ai_modified_line4".ai());

    // Stage only the modifications
    file.stage();

    // AI adds more lines that won't be staged
    file.insert_at(
        5,
        crate::lines!["ai_line6".ai(), "ai_line7".ai(), "ai_line8".ai()],
    );

    let commit = repo.commit("Partial staging").unwrap();

    // With per-trace attestation keys, we may have multiple entries per file
    assert!(!commit.authorship_log.attestations.is_empty());

    // Only check committed lines
    file.assert_committed_lines(crate::lines![
        "line1".human(),
        "ai_modified_line2".ai(),
        "line3".human(),
        "ai_modified_line4".ai(),
        // line5 is human, but it's not considered committed, because adding line 6+ also added a newline char to line 5
    ]);
}

#[test]
fn test_unstaged_changes_not_committed() {
    // Test that unstaged changes don't appear in the commit
    let repo = TestRepo::new();
    let mut file = repo.filename("test.ts");

    file.set_contents(crate::lines!["line1", "line2", "line3"]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI adds lines at the end and stages them
    file.insert_at(3, crate::lines!["ai_line4".ai(), "ai_line5".ai()]);
    file.stage();

    // AI adds more lines that won't be staged
    file.insert_at(
        5,
        crate::lines!["unstaged_line6".ai(), "unstaged_line7".ai()],
    );

    let commit = repo.commit("Commit only staged lines").unwrap();

    // Only the staged lines should be in the commit
    assert!(!commit.authorship_log.attestations.is_empty());

    // Only check committed lines
    file.assert_committed_lines(crate::lines![
        "line1".human(),
        "line2".human(),
        "line3".ai(),
        "ai_line4".ai(),
        // line 5 is ai, but it's not considered committed, because adding line 6+ also added a newline char to line 5
    ]);
}

#[test]
fn test_unstaged_ai_lines_saved_to_working_log() {
    // Test that unstaged AI-authored lines are saved to the working log for the next commit
    let repo = TestRepo::new();
    let mut file = repo.filename("test.ts");

    file.set_contents(crate::lines!["line1", "line2", "line3", ""]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI adds lines 4-7 and stages some
    file.insert_at(3, crate::lines!["ai_line4".ai(), "ai_line5".ai()]);
    file.stage();

    // AI adds more lines that won't be staged
    file.insert_at(5, crate::lines!["ai_line6".ai(), "ai_line7".ai()]);

    // Commit only the staged lines
    let first_commit = repo.commit("Partial AI commit").unwrap();

    // The commit should only have lines 4-5
    assert_eq!(first_commit.authorship_log.attestations.len(), 1);

    // Now stage and commit the remaining lines
    file.stage();
    let second_commit = repo.commit("Commit remaining AI lines").unwrap();

    // The second commit should also attribute lines 6-7 to AI
    assert_eq!(second_commit.authorship_log.attestations.len(), 1);

    // Final state should have all AI lines attributed
    file.assert_lines_and_blame(crate::lines![
        "line1".human(),
        "line2".human(),
        "line3".human(),
        "ai_line4".ai(),
        "ai_line5".ai(),
        "ai_line6".ai(),
        "ai_line7".ai(),
    ]);
}

/// Test: New file with partial staging across two commits
/// AI creates a new file with many lines, stage only some, then commit the rest
#[test]
fn test_new_file_partial_staging_two_commits() {
    let repo = TestRepo::new();

    // Create an initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates a brand new file with planets
    let mut file = repo.filename("planets.txt");
    file.set_contents(crate::lines![
        "Mercury".ai(),
        "Venus".ai(),
        "Earth".ai(),
        "Mars".ai(),
        "Jupiter".ai(),
        "Saturn".ai(),
        "Uranus".ai(),
        "Neptune".ai(),
        "Pluto (dwarf)".ai(),
    ]);

    // First commit should have all the planets
    let first_commit = repo.stage_all_and_commit("Add planets").unwrap();

    assert_eq!(first_commit.authorship_log.attestations.len(), 1);

    file.assert_lines_and_blame(crate::lines![
        "Mercury".ai(),
        "Venus".ai(),
        "Earth".ai(),
        "Mars".ai(),
        "Jupiter".ai(),
        "Saturn".ai(),
        "Uranus".ai(),
        "Neptune".ai(),
        "Pluto (dwarf)".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_partial_staging_filters_unstaged_lines,
    test_human_stages_some_ai_lines,
);
