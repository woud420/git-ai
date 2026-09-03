use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::authorship_log_serialization::generate_short_hash;
use git_ai::operations::git::repo_storage::PersistedWorkingLog;
use git_ai::operations::git::repository as GitAiRepository;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;

fn checkpoint_working_log(repo: &TestRepo, commit_sha: &str) -> PersistedWorkingLog {
    GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("find repository")
        .storage
        .working_log_for_base_commit(commit_sha)
        .unwrap()
}

fn truncate_checkpoint_hashes(repo: &TestRepo, commit_sha: &str) -> HashSet<String> {
    let working_log = checkpoint_working_log(repo, commit_sha);
    let checkpoint_file = working_log.checkpoints_file();

    if !checkpoint_file.exists() {
        return HashSet::new();
    }

    let mut short_ids = HashSet::new();
    let mut modified_lines = Vec::new();
    for mut checkpoint in working_log
        .read_all_checkpoints()
        .expect("Failed to read checkpoints")
    {
        let short_id = checkpoint.agent_id.as_ref().map(|agent| {
            let prompt_id = generate_short_hash(&agent.id, &agent.tool);
            prompt_id[..7].to_string()
        });
        checkpoint.trace_id = None;
        let mut checkpoint = serde_json::to_value(checkpoint).unwrap();

        // Modify entries in the checkpoint
        if let Some(entries) = checkpoint.get_mut("entries").and_then(|e| e.as_array_mut()) {
            for entry in entries {
                // Truncate author_ids in attributions
                if let Some(attributions) =
                    entry.get_mut("attributions").and_then(|a| a.as_array_mut())
                {
                    for attr in attributions {
                        if let Some(author_id) =
                            attr.get_mut("author_id").and_then(|id| id.as_str())
                            && author_id != "human"
                            && let Some(short_id) = &short_id
                        {
                            short_ids.insert(short_id.clone());
                            attr["author_id"] = Value::String(short_id.clone());
                        }
                    }
                }

                // Truncate author_ids in line_attributions
                if let Some(line_attrs) = entry
                    .get_mut("line_attributions")
                    .and_then(|a| a.as_array_mut())
                {
                    for line_attr in line_attrs {
                        if let Some(author_id) =
                            line_attr.get_mut("author_id").and_then(|id| id.as_str())
                            && author_id != "human"
                            && let Some(short_id) = &short_id
                        {
                            short_ids.insert(short_id.clone());
                            line_attr["author_id"] = Value::String(short_id.clone());
                        }
                        if let Some(overrode) =
                            line_attr.get_mut("overrode").and_then(|o| o.as_str())
                            && overrode != "human"
                            && let Some(short_id) = &short_id
                        {
                            short_ids.insert(short_id.clone());
                            line_attr["overrode"] = Value::String(short_id.clone());
                        }
                    }
                }
            }
        }

        modified_lines
            .push(serde_json::to_string(&checkpoint).expect("Failed to serialize checkpoint"));
    }

    // Write back the modified checkpoints
    let new_content = modified_lines.join("\n") + "\n";
    fs::write(&checkpoint_file, new_content).expect("Failed to write modified checkpoint file");
    short_ids
}

fn assert_quoted_short_ids_absent(raw_journal: &str, short_ids: &HashSet<String>) {
    for short_id in short_ids {
        assert!(
            !raw_journal.contains(&format!("\"{short_id}\"")),
            "raw checkpoint journal still contains short prompt ID {short_id}"
        );
    }
}

#[test]
fn raw_short_id_assertion_detects_an_injected_id() {
    let result = std::panic::catch_unwind(|| {
        assert_quoted_short_ids_absent(
            r#"{"author_id":"1234567"}"#,
            &HashSet::from(["1234567".to_string()]),
        );
    });

    assert!(result.is_err(), "control must reject a quoted short ID");
}

/// Verify that all IDs in an authorship log use the correct format.
/// Session IDs are `s_` + 14 hex = 16 chars. Attestation hashes are either
/// `s_14hex::t_14hex` (34 chars) for session format or 16 chars for old prompt format.
fn verify_prompt_ids_are_16_chars(
    authorship_log: &git_ai::model::authorship_log_serialization::AuthorshipLog,
) {
    for session_id in authorship_log.metadata.sessions.keys() {
        assert_eq!(
            session_id.len(),
            16,
            "Session ID '{}' should be 16 chars long, but is {} chars",
            session_id,
            session_id.len()
        );
    }

    for prompt_id in authorship_log.metadata.prompts.keys() {
        assert_eq!(
            prompt_id.len(),
            16,
            "Prompt ID '{}' should be 16 chars long, but is {} chars",
            prompt_id,
            prompt_id.len()
        );
    }

    for attestation in &authorship_log.attestations {
        for entry in &attestation.entries {
            let valid_len = if entry.hash.starts_with("s_") {
                entry.hash.len() == 34 || entry.hash.len() == 16
            } else if entry.hash.starts_with("h_") {
                true
            } else {
                entry.hash.len() == 16
            };
            assert!(
                valid_len,
                "Attestation hash '{}' has unexpected length {} chars",
                entry.hash,
                entry.hash.len()
            );
        }
    }
}

fn verify_checkpoint_hashes_are_migrated(
    repo: &TestRepo,
    commit_sha: &str,
    injected_short_ids: &HashSet<String>,
) {
    let working_log = checkpoint_working_log(repo, commit_sha);
    let checkpoint_file = working_log.checkpoints_file();

    if !checkpoint_file.exists() {
        return;
    }

    let raw_journal = fs::read_to_string(&checkpoint_file).expect("read raw checkpoint journal");
    assert_quoted_short_ids_absent(&raw_journal, injected_short_ids);

    for checkpoint in working_log
        .read_all_checkpoints()
        .expect("Failed to read checkpoints")
    {
        for entry in checkpoint.entries {
            for attribution in entry.attributions {
                if attribution.author_id != "human" {
                    assert!(
                        matches!(attribution.author_id.len(), 16 | 34),
                        "Author ID '{}' in attributions should use prompt or session format after migration, but is {} chars",
                        attribution.author_id,
                        attribution.author_id.len()
                    );
                }
            }

            for attribution in entry.line_attributions {
                if attribution.author_id != "human" {
                    assert!(
                        matches!(attribution.author_id.len(), 16 | 34),
                        "Author ID '{}' in line_attributions should use prompt or session format after migration, but is {} chars",
                        attribution.author_id,
                        attribution.author_id.len()
                    );
                }
                if let Some(overrode) = attribution.overrode
                    && overrode != "human"
                {
                    assert!(
                        matches!(overrode.len(), 16 | 34),
                        "Overrode ID '{}' should use prompt or session format after migration, but is {} chars",
                        overrode,
                        overrode.len()
                    );
                }
            }
        }
    }
}

#[test]
fn test_prompt_hash_migration_ai_adds_lines_multiple_commits() {
    // Test AI adding lines across multiple commits
    let repo = TestRepo::new();
    let mut file = repo.filename("test.ts");

    file.set_contents(crate::lines!["base_line", ""]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    file.insert_at(
        1,
        crate::lines!["ai_line1".ai(), "ai_line2".ai(), "ai_line3".ai(),],
    );

    let first_commit = repo.stage_all_and_commit("AI adds first batch").unwrap();
    let first_commit_sha = &first_commit.commit_sha;

    file.insert_at(4, crate::lines!["ai_line4".ai()]);

    let injected_short_ids = truncate_checkpoint_hashes(&repo, first_commit_sha);
    assert!(!injected_short_ids.is_empty());

    file.insert_at(5, crate::lines!["ai_line5".ai()]);
    verify_checkpoint_hashes_are_migrated(&repo, first_commit_sha, &injected_short_ids);

    let second_commit = repo.stage_all_and_commit("AI adds second batch").unwrap();

    // Verify that all prompt IDs are 16 chars in both commits
    verify_prompt_ids_are_16_chars(&first_commit.authorship_log);
    verify_prompt_ids_are_16_chars(&second_commit.authorship_log);

    file.assert_lines_and_blame(crate::lines![
        "base_line".human(),
        "ai_line1".ai(),
        "ai_line2".ai(),
        "ai_line3".ai(),
        "ai_line4".ai(),
        "ai_line5".ai(),
    ]);
}

#[test]
fn test_prompt_hash_migration_ai_adds_then_commits_in_batches() {
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

    let first_commit = repo.commit("Add lines 5-7").unwrap();
    let first_commit_sha = &first_commit.commit_sha;

    file.insert_at(7, crate::lines!["ai_line8".ai(), "ai_line9".ai()]);

    let injected_short_ids = truncate_checkpoint_hashes(&repo, first_commit_sha);
    assert!(!injected_short_ids.is_empty());

    file.insert_at(9, crate::lines!["ai_line10".ai()]);
    verify_checkpoint_hashes_are_migrated(&repo, first_commit_sha, &injected_short_ids);

    let second_commit = repo.stage_all_and_commit("Add lines 8-10").unwrap();

    // Verify that all prompt IDs are 16 chars in both commits
    verify_prompt_ids_are_16_chars(&first_commit.authorship_log);
    verify_prompt_ids_are_16_chars(&second_commit.authorship_log);

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
fn test_prompt_hash_migration_unstaged_ai_lines_saved_to_working_log() {
    // Test that unstaged AI-authored lines are saved to the working log for the next commit
    let repo = TestRepo::new();
    let mut file = repo.filename("test.ts");

    file.set_contents(crate::lines!["line1", "line2", "line3", ""]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI adds lines 4-7 and stages some
    file.insert_at(3, crate::lines!["ai_line4".ai(), "ai_line5".ai()]);
    file.stage();

    // Commit only the staged lines
    let first_commit = repo.commit("Partial AI commit").unwrap();
    let first_commit_sha = &first_commit.commit_sha;

    // The commit should only have lines 4-5
    assert_eq!(first_commit.authorship_log.attestations.len(), 1);

    file.insert_at(5, crate::lines!["ai_line6".ai()]);

    let injected_short_ids = truncate_checkpoint_hashes(&repo, first_commit_sha);
    assert!(!injected_short_ids.is_empty());

    file.insert_at(6, crate::lines!["ai_line7".ai()]);
    verify_checkpoint_hashes_are_migrated(&repo, first_commit_sha, &injected_short_ids);

    // Now stage and commit the remaining lines
    file.stage();
    let second_commit = repo.commit("Commit remaining AI lines").unwrap();

    // The second commit should also attribute lines 6-7 to AI
    assert_eq!(second_commit.authorship_log.attestations.len(), 1);

    // Verify that after migration, all prompt IDs are 16 chars
    verify_prompt_ids_are_16_chars(&first_commit.authorship_log);
    verify_prompt_ids_are_16_chars(&second_commit.authorship_log);

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

crate::reuse_tests_in_worktree!(
    test_prompt_hash_migration_ai_adds_lines_multiple_commits,
    test_prompt_hash_migration_ai_adds_then_commits_in_batches,
    test_prompt_hash_migration_unstaged_ai_lines_saved_to_working_log,
);
