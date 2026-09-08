use super::{ExpectedLineExt, HashMap, TestRepo};

/// Test amending a commit by adding AI-authored lines at the top of the file.
#[test]
fn test_amend_add_lines_at_top() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Initial file with human content
    file.set_contents(crate::lines![
        "line 1", "line 2", "line 3", "line 4", "line 5"
    ]);

    repo.git(&["add", "-A"]).unwrap();

    repo.commit("Initial commit").unwrap();

    // AI adds lines at the top
    file.insert_at(
        0,
        crate::lines!["// AI added line 1".ai(), "// AI added line 2".ai()],
    );

    // Amend the commit WITHOUT staging the AI lines
    repo.git(&["commit", "--amend", "-m", "Initial commit (amended)"])
        .unwrap();

    // Now stage and commit the AI lines
    repo.stage_all_and_commit("Add AI lines").unwrap();

    // Verify AI authorship is preserved after the second commit
    file.assert_lines_and_blame(crate::lines![
        "// AI added line 1".ai(),
        "// AI added line 2".ai(),
        "line 1".human(),
        "line 2".human(),
        "line 3".human(),
        "line 4".human(),
        "line 5".human()
    ]);
}

#[test]
fn test_amend_add_lines_in_middle() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Initial file with human content
    file.set_contents(crate::lines![
        "line 1", "line 2", "line 3", "line 4", "line 5"
    ]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI adds lines in the middle
    file.insert_at(
        2,
        crate::lines!["// AI inserted line 1".ai(), "// AI inserted line 2".ai()],
    );

    // Amend the commit
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "--amend", "-m", "Initial commit (amended)"])
        .unwrap();

    // Verify AI authorship is preserved
    file.assert_lines_and_blame(crate::lines![
        "line 1".human(),
        "line 2".human(),
        "// AI inserted line 1".ai(),
        "// AI inserted line 2".ai(),
        "line 3".human(),
        "line 4".human(),
        "line 5".human()
    ]);
}

#[test]
fn test_amend_add_lines_at_bottom() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Initial file with human content
    file.set_contents(crate::lines![
        "line 1", "line 2", "line 3", "line 4", "line 5"
    ]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI adds lines at the bottom
    file.insert_at(
        5,
        crate::lines!["// AI appended line 1".ai(), "// AI appended line 2".ai()],
    );

    // Amend the commit
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "--amend", "-m", "Initial commit (amended)"])
        .unwrap();

    // Verify AI authorship is preserved
    file.assert_lines_and_blame(crate::lines![
        "line 1".human(),
        "line 2".human(),
        "line 3".human(),
        "line 4".human(),
        "line 5".ai(),
        "// AI appended line 1".ai(),
        "// AI appended line 2".ai()
    ]);
}

#[test]
fn test_amend_multiple_changes() {
    let repo = TestRepo::new();
    let mut file = repo.filename("code.js");

    // Initial file with AI content
    file.set_contents(crate::lines![
        "function example() {".ai(),
        "  return 42;".ai(),
        "}".ai()
    ]);
    repo.stage_all_and_commit("Add example function").unwrap();

    // AI adds header comment
    file.insert_at(0, crate::lines!["// Header comment".ai()]);
    // After inserting at 0, the file now has 4 lines

    // AI adds documentation in middle (after line 2: "function example() {")
    file.insert_at(2, crate::lines!["  // Added documentation".ai()]);
    // After inserting at 2, the file now has 5 lines

    // AI adds footer at bottom (at the end after "}")
    file.insert_at(5, crate::lines!["// Footer".ai()]);

    // Amend the commit
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "--amend", "-m", "Add example function (amended)"])
        .unwrap();

    // Verify all AI authorship is preserved
    file.assert_lines_and_blame(crate::lines![
        "// Header comment".ai(),
        "function example() {".ai(),
        "  // Added documentation".ai(),
        "  return 42;".ai(),
        "}".ai(),
        "// Footer".ai()
    ]);
}

#[test]
fn test_amend_repeated_round_trips_preserve_exact_line_authorship() {
    let repo = TestRepo::new();
    let mut file = repo.filename("code.js");

    file.set_contents(crate::lines![
        "function example() {".ai(),
        "  return 42;".ai(),
        "}".ai()
    ]);
    repo.stage_all_and_commit("Add example function").unwrap();

    file.insert_at(0, crate::lines!["// Header comment".ai()]);
    file.insert_at(2, crate::lines!["  // Added documentation".ai()]);
    file.insert_at(5, crate::lines!["// Footer".ai()]);
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&[
        "commit",
        "--amend",
        "-m",
        "Add example function (amended 1)",
    ])
    .unwrap();

    // Re-amend the same commit with mixed authorship changes.
    file.insert_at(0, crate::lines!["// Human TODO".human()]);
    file.insert_at(7, crate::lines!["// AI trailing note".ai()]);
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&[
        "commit",
        "--amend",
        "-m",
        "Add example function (amended 2)",
    ])
    .unwrap();

    file.assert_lines_and_blame(crate::lines![
        "// Human TODO".human(),
        "// Header comment".ai(),
        "function example() {".ai(),
        "  // Added documentation".ai(),
        "  return 42;".ai(),
        "}".ai(),
        "// Footer".ai(),
        "// AI trailing note".ai()
    ]);
}

/// Test that custom attributes set via config are preserved through an amend
/// when the real post-commit pipeline injects them.
#[test]
fn test_amend_preserves_custom_attributes_from_config() {
    let mut repo = TestRepo::new_dedicated_daemon();

    // Configure custom attributes via config patch
    let mut attrs = HashMap::new();
    attrs.insert("employee_id".to_string(), "E202".to_string());
    attrs.insert("team".to_string(), "security".to_string());
    repo.patch_git_ai_config(|patch| {
        patch.custom_attributes = Some(attrs.clone());
    });

    // Create initial commit with AI content
    let mut file = repo.filename("code.txt");
    file.set_contents(crate::lines![
        "// AI generated code".ai(),
        "function init() {}".ai()
    ]);
    repo.stage_all_and_commit("Initial AI commit").unwrap();

    // Verify custom attributes were set on the original commit
    let original_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let original_log = repo.require_authorship_log(&original_sha);
    assert!(
        original_log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !original_log.metadata.sessions.is_empty(),
        "precondition: original commit should have session records"
    );
    for session in original_log.metadata.sessions.values() {
        assert_eq!(
            session.custom_attributes.as_ref(),
            Some(&attrs),
            "precondition: original commit should have custom_attributes from config (sessions)"
        );
    }

    // Amend the commit with additional AI lines
    file.insert_at(2, crate::lines!["// More AI code".ai()]);
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "--amend", "-m", "Initial AI commit (amended)"])
        .unwrap();

    // Verify custom attributes survived the amend
    let amended_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let amended_log = repo.require_authorship_log(&amended_sha);
    assert!(
        amended_log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !amended_log.metadata.sessions.is_empty(),
        "amended commit should have session records"
    );
    for session in amended_log.metadata.sessions.values() {
        assert_eq!(
            session.custom_attributes.as_ref(),
            Some(&attrs),
            "custom_attributes should be preserved through amend (sessions)"
        );
    }

    // Also verify the AI attribution itself survived
    file.assert_lines_and_blame(crate::lines![
        "// AI generated code".ai(),
        "function init() {}".ai(),
        "// More AI code".ai()
    ]);
}

/// Bug regression: amend a commit and delete the AI-authored line.
/// The amended note should NOT contain a prompt record for the deleted AI line.
///
/// Before the fix, `to_authorship_log_and_initial_working_log` copied ALL prompts from
/// VirtualAttributions upfront without pruning them to only those referenced by
/// actual attestations.  When an AI line was deleted in the amend the attestation
/// was correctly absent, but the orphaned PromptRecord remained in the metadata.
#[test]
fn test_amend_delete_ai_line_removes_prompt_from_note() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create a commit that contains both human and AI lines.
    file.set_contents(crate::lines![
        "human line 1",
        "// AI authored line".ai(),
        "human line 2"
    ]);
    repo.stage_all_and_commit("Initial commit with AI line")
        .unwrap();

    let original_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let original_log = repo.require_authorship_log(&original_sha);
    assert!(
        original_log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    assert!(
        !original_log.metadata.sessions.is_empty(),
        "precondition: original commit should have session records"
    );

    // Amend: overwrite the file with only human content, deleting the AI line.
    let file_path = repo.path().join("test.txt");
    std::fs::write(&file_path, "human line 1\nhuman line 2\n").unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "--amend", "-m", "Amended - AI line deleted"])
        .unwrap();

    let amended_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let amended_log = repo.require_authorship_log(&amended_sha);

    assert!(
        amended_log.metadata.prompts.is_empty(),
        "amended note should have no prompts since the only AI line was deleted, \
         but found orphaned prompts: {:?}",
        amended_log.metadata.prompts.keys().collect::<Vec<_>>()
    );
    assert!(
        amended_log.metadata.sessions.is_empty(),
        "amended note should have no sessions since the only AI line was deleted, \
         but found orphaned sessions: {:?}",
        amended_log.metadata.sessions.keys().collect::<Vec<_>>()
    );
}

/// Bug regression (worse variant): amend a commit and delete an AI-authored line that
/// was originally introduced by an *earlier* commit.
///
/// When the blame on the pre-amend commit surfaces prompt IDs from older commits,
/// those foreign PromptRecords must NOT appear in the amended commit's note.
/// Before the fix the note for the amended commit contained the earlier commit's
/// PromptRecord even though it had no corresponding attestation.
#[test]
fn test_amend_delete_prior_commit_ai_line_no_foreign_prompt_in_note() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Commit A: introduces an AI line (prompt P1) and a human line.
    file.set_contents(crate::lines![
        "// AI authored line from commit A".ai(),
        "human line from commit A"
    ]);
    repo.stage_all_and_commit("Commit A with AI line").unwrap();

    let commit_a_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let commit_a_log = repo.require_authorship_log(&commit_a_sha);
    assert!(
        commit_a_log.metadata.prompts.is_empty(),
        "new-format test should produce sessions, not prompts"
    );
    let commit_a_session_ids: Vec<String> =
        commit_a_log.metadata.sessions.keys().cloned().collect();
    assert!(
        !commit_a_session_ids.is_empty(),
        "precondition: commit A should have session records"
    );

    // Commit B: a human-only addition on top of A.
    // We write directly to avoid creating AI checkpoints for B.
    let file_path = repo.path().join("test.txt");
    std::fs::write(
        &file_path,
        "// AI authored line from commit A\nhuman line from commit A\nhuman line from commit B\n",
    )
    .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "-m", "Commit B - human addition"])
        .unwrap();

    // Amend commit B: delete the AI line that came from commit A.
    // After the amend, the file contains only human lines.
    std::fs::write(
        &file_path,
        "human line from commit A\nhuman line from commit B\n",
    )
    .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&[
        "commit",
        "--amend",
        "-m",
        "Commit B amended - also deleted AI from A",
    ])
    .unwrap();

    let amended_b_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let amended_b_log = repo.require_authorship_log(&amended_b_sha);

    // The amended B note must NOT contain any of commit A's session IDs.
    // They are foreign to commit B and have no corresponding attestation.
    assert!(
        amended_b_log.metadata.prompts.is_empty(),
        "amended B should have no prompts"
    );
    for session_id in &commit_a_session_ids {
        assert!(
            !amended_b_log.metadata.sessions.contains_key(session_id),
            "Amended B's note should not contain session '{}' from commit A \
             (foreign-session-leak bug): amended_b sessions = {:?}",
            session_id,
            amended_b_log.metadata.sessions.keys().collect::<Vec<_>>()
        );
    }
}

/// Amending a commit and deleting a KnownHuman-attributed line must preserve the
/// HumanRecord in the note's `metadata.humans`.
///
/// The note is a historical record of every contributor that touched the commit.
/// Deleting the attributed line removes the *attribution* (line coordinates), but
/// the HumanRecord itself must remain — matching how PromptRecords are preserved
/// via `checkpoint_prompt_ids` even when all attributed AI lines are deleted.
#[test]
fn test_amend_delete_known_human_line_preserves_human_record_in_note() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Create a commit that contains a mix of human-attributed and plain human lines.
    // Using `.human()` triggers a `checkpoint mock_known_human` which stores an
    // h_-prefixed HumanRecord in the note's metadata.humans.
    file.set_contents(crate::lines![
        "regular human line",
        "// KnownHuman attested line".human(),
        "another regular line"
    ]);
    repo.stage_all_and_commit("Initial commit with KnownHuman line")
        .unwrap();

    let original_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let original_log = repo.require_authorship_log(&original_sha);
    assert!(
        !original_log.metadata.humans.is_empty(),
        "precondition: original commit should have HumanRecord entries"
    );
    let original_human_ids: Vec<String> = original_log.metadata.humans.keys().cloned().collect();

    // Amend: overwrite the file with plain human content only, deleting the KnownHuman line.
    let file_path = repo.path().join("test.txt");
    std::fs::write(&file_path, "regular human line\nanother regular line\n").unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&[
        "commit",
        "--amend",
        "-m",
        "Amended - KnownHuman line deleted",
    ])
    .unwrap();

    let amended_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let amended_log = repo.require_authorship_log(&amended_sha);

    // The HumanRecord must survive the amend even though its attributed line was deleted.
    // The note is a commit-level record of contributors; removing a line doesn't erase
    // the contributor's association with the commit.
    assert!(
        !amended_log.metadata.humans.is_empty(),
        "amended note should still contain the HumanRecord(s) from the original commit \
         even though the KnownHuman line was deleted; got: {:?}",
        amended_log.metadata.humans.keys().collect::<Vec<_>>()
    );
    for id in &original_human_ids {
        assert!(
            amended_log.metadata.humans.contains_key(id),
            "HumanRecord '{}' present in original note must be preserved after amend; \
             amended note has: {:?}",
            id,
            amended_log.metadata.humans.keys().collect::<Vec<_>>()
        );
    }
}

crate::reuse_tests_in_worktree!(
    test_amend_add_lines_at_top,
    test_amend_add_lines_in_middle,
    test_amend_add_lines_at_bottom,
    test_amend_multiple_changes,
    test_amend_repeated_round_trips_preserve_exact_line_authorship,
    test_amend_delete_ai_line_removes_prompt_from_note,
    test_amend_delete_prior_commit_ai_line_no_foreign_prompt_in_note,
    test_amend_delete_known_human_line_preserves_human_record_in_note,
);
