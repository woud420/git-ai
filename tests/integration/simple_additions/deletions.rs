use super::{Attribution, CheckpointKind, ExpectedLineExt, TestRepo, WorkingLogEntry, fs};

#[test]
fn test_simple_ai_then_human_deletion() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1", "Line 2", "Line 3", "Line 4", "Line 5"
    ]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    file.insert_at(5, crate::lines!["AI Line".ai()]);

    repo.stage_all_and_commit("AI adds line").unwrap();

    file.delete_at(5);

    let commit = repo.stage_all_and_commit("Human deletes AI line").unwrap();

    // The authorship log should have no attestations since we only deleted lines
    assert_eq!(commit.authorship_log.attestations.len(), 0);

    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "Line 2".human(),
        "Line 3".human(),
        "Line 4".human(),
        "Line 5".human(),
    ]);
}

#[test]
fn test_multiple_ai_checkpoints_with_human_deletions() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    // Two initial lines: "Base" stays human (not adjacent to AI hunks);
    // "Base2" (last line) gets pulled into the AI hunk and becomes AI.
    file.set_contents(crate::lines!["Base", "Base2"]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    file.insert_at(2, crate::lines!["AI1 Line 1".ai(), "AI1 Line 2".ai()]);
    file.insert_at(4, crate::lines!["AI2 Line 1".ai(), "AI2 Line 2".ai()]);

    // Delete the first AI session's lines (indices 2 and 3)
    file.delete_range(2, 4);

    let commit = repo.stage_all_and_commit("Complex commit").unwrap();

    // Should only have AI2's lines attributed (now at indices 2 and 3 after deletion)
    assert_eq!(commit.authorship_log.attestations.len(), 1);

    // "Base" stays human — it's not at the hunk boundary.
    // "Base2" becomes AI — it was the last line in the original, so force_split
    // places it in the same 1→N hunk as the AI insertions.
    file.assert_lines_and_blame(crate::lines![
        "Base".human(),
        "Base2".ai(),
        "AI2 Line 1".ai(),
        "AI2 Line 2".ai(),
    ]);
}

#[test]
fn test_complex_mixed_additions_and_deletions() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1", "Line 2", "Line 3", "Line 4", "Line 5", "Line 6", "Line 7", "Line 8", "Line 9",
        "Line 10",
    ]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI deletes lines 2-3 and replaces with new content (delete at index 1, 2 items)
    file.delete_range(1, 3);
    file.insert_at(
        1,
        crate::lines!["NEW LINE A".ai(), "NEW LINE B".ai(), "NEW LINE C".ai(),],
    );

    // AI inserts at the end
    file.insert_at(11, crate::lines!["END LINE 1".ai(), "END LINE 2".ai(),]);

    let commit = repo.stage_all_and_commit("Complex edits").unwrap();

    // Should have lines 2-4 and the last 2 lines attributed to AI
    assert_eq!(commit.authorship_log.attestations.len(), 1);

    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "NEW LINE A".ai(),
        "NEW LINE B".ai(),
        "NEW LINE C".ai(),
        "Line 4".human(),
        "Line 5".human(),
        "Line 6".human(),
        "Line 7".human(),
        "Line 8".human(),
        "Line 9".human(),
        "Line 10".ai(),
        "END LINE 1".ai(),
        "END LINE 2".ai(),
    ]);
}

#[test]
fn test_ai_deletion_with_human_checkpoint_in_same_commit() {
    // Regression test for issue #193
    // When both human and AI checkpoints happen in the same commit,
    // and AI deletes its own lines, human additions should still be
    // attributed correctly (not claimed by AI)
    use std::fs;

    let repo = TestRepo::new();
    let file_path = repo.path().join("data.txt");

    repo.human_edit("data.txt", "Base Line 1\nBase Line 2\nBase Line 3");

    fs::write(
        &file_path,
        "Base Line 1\nBase Line 2\nAI: Line 1\nAI: Line 2\nAI: Line 3\nBase Line 3",
    )
    .unwrap();

    // Mark only the AI lines with mock_ai checkpoint
    repo.git_ai(&["checkpoint", "mock_ai", "data.txt"]).unwrap();

    repo.stage_all_and_commit("Commit 1: AI adds 3 lines")
        .unwrap();

    // COMMIT 2: Human adds 2 lines, then AI modifies
    // -------
    // Step 1: Human adds lines
    fs::write(
        &file_path,
        "Base Line 1\nBase Line 2\nAI: Line 1\nAI: Line 2\nAI: Line 3\nHuman: Line 1\nHuman: Line 2\nBase Line 3",
    )
    .unwrap();

    // KnownHuman checkpoint for the human-added lines
    repo.git_ai(&["checkpoint", "mock_known_human", "data.txt"])
        .unwrap();

    // Step 2: AI deletes one of its own lines and adds 2 new lines
    fs::write(
        &file_path,
        "Base Line 1\nBase Line 2\nAI: Line 1\nAI: Line 3\nHuman: Line 1\nHuman: Line 2\nAI: New Line 1\nAI: New Line 2\nBase Line 3",
    )
    .unwrap();

    // AI checkpoint
    println!(
        "checkpoint: {}",
        repo.git_ai(&["checkpoint", "mock_ai", "data.txt"]).unwrap()
    );

    // Now commit everything together
    let commit = repo
        .stage_all_and_commit("Commit 2: Human adds 2, AI deletes 1 and adds 2")
        .unwrap();

    commit.print_authorship();

    println!("file: {:?}", repo.git_ai(&["blame", "data.txt"]).unwrap());

    // Verify line-by-line attribution
    let mut file = repo.filename("data.txt");
    file.assert_lines_and_blame(crate::lines![
        "Base Line 1".human(),
        "Base Line 2".human(),
        "AI: Line 1".ai(),
        "AI: Line 3".ai(),
        "Human: Line 1".human(), // Should be human, not AI (Bug #193)
        "Human: Line 2".human(), // Should be human, not AI (Bug #193)
        "AI: New Line 1".ai(),
        "AI: New Line 2".ai(),
        "Base Line 3".human(),
    ]);

    // Verify the stats are correct for the last commit
    let stats_output = repo.git_ai(&["stats", "HEAD", "--json"]).unwrap();
    let stats_output = stats_output.split("}}}").next().unwrap().to_string() + "}}}";
    let stats: serde_json::Value = serde_json::from_str(&stats_output).unwrap();

    // Expected: 2 human additions, 2 AI additions
    // Bug #193 causes: 0 human additions, 4 AI additions
    assert_eq!(
        stats["human_additions"].as_u64().unwrap(),
        2,
        "Human additions should be 2, not 0 (Bug #193)"
    );
    assert_eq!(
        stats["ai_additions"].as_u64().unwrap(),
        2,
        "AI additions should be 2, not 4 (Bug #193)"
    );
}

#[test]
fn test_deletion_within_a_single_line_attribution() {
    // Regression test for bug where removing a constructor parameter
    // doesn't get attributed to AI when using mock_ai checkpoint
    // This replicates the scenario where:
    // - constructor(_config: Config, enabled: boolean = true) { [no-data]
    // + constructor(enabled: boolean = true) { [no-data]
    // The constructor line should be attributed to AI
    use std::fs;

    let repo = TestRepo::new();
    let file_path = repo.path().join("git-ai-integration-service.ts");

    // Initial commit: File with old constructor signature (all human)
    fs::write(
        &file_path,
        "/**\n * Service for integrating git-ai hooks into the hook system.\n */\nexport class GitAiIntegrationService {\n  private readonly commandPath: string;\n  private registered = false;\n\n  constructor(_config: Config, enabled: boolean = true) {\n    this.enabled = enabled;\n    this.commandPath = 'git-ai';\n  }\n}\n",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial commit with old constructor")
        .unwrap();

    // Second commit: AI removes the _config parameter
    fs::write(
        &file_path,
        "/**\n * Service for integrating git-ai hooks into the hook system.\n */\nexport class GitAiIntegrationService {\n  private readonly commandPath: string;\n  private registered = false;\n\n  constructor(enabled: boolean = true) {\n    this.enabled = enabled;\n    this.commandPath = 'git-ai';\n  }\n}\n",
    )
    .unwrap();

    // Mark the change as AI-authored
    repo.git_ai(&["checkpoint", "mock_ai", "git-ai-integration-service.ts"])
        .unwrap();

    repo.stage_all_and_commit("AI removes constructor parameter")
        .unwrap();

    // Verify line-by-line attribution - the constructor line should be AI
    let mut file = repo.filename("git-ai-integration-service.ts");
    file.assert_lines_and_blame(crate::lines![
        "/**".human(),
        " * Service for integrating git-ai hooks into the hook system.".human(),
        " */".human(),
        "export class GitAiIntegrationService {".human(),
        "  private readonly commandPath: string;".human(),
        "  private registered = false;".human(),
        "".human(),
        "  constructor(enabled: boolean = true) {".ai(), // Should be AI, not [no-data]
        "    this.enabled = enabled;".human(),
        "    this.commandPath = 'git-ai';".human(),
        "  }".human(),
        "}".human(),
    ]);
}

#[test]
fn test_deletion_of_multiple_lines_by_ai() {
    // Regression test for bug where removing a constructor parameter
    // doesn't get attributed to AI when using mock_ai checkpoint
    // This replicates the scenario where:
    // - constructor(_config: Config, enabled: boolean = true) { [no-data]
    // + constructor(enabled: boolean = true) { [no-data]
    // The constructor line should be attributed to AI
    use std::fs;

    let repo = TestRepo::new();
    let file_path = repo.path().join("git-ai-integration-service.ts");

    // Initial commit: File with old constructor signature (all human)
    fs::write(
        &file_path,
        "/**\n * Service for integrating git-ai hooks into the hook system.\n */\nexport class GitAiIntegrationService {\n  private readonly commandPath: string;\n  private registered = false;\n\n  constructor(_config: Config, enabled: boolean = true) {\n    this.enabled = enabled;\n    this.commandPath = 'git-ai';\n  }\n}\n",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial commit with old constructor")
        .unwrap();

    // Second commit: AI removes the _config parameter
    fs::write(
        &file_path,
        "/**\n * Service for integrating git-ai hooks into the hook system.\n */\nexport class GitAiIntegrationService {\n  private readonly commandPath: string;\n  constructor(_config: Config, enabled: boolean = true) {\n    this.commandPath = 'git-ai';\n  }\n}\n",
    )
    .unwrap();

    // Mark the change as AI-authored
    repo.git_ai(&["checkpoint", "mock_ai", "git-ai-integration-service.ts"])
        .unwrap();

    repo.stage_all_and_commit("AI removes constructor parameter")
        .unwrap();

    // Verify line-by-line attribution - the constructor line should be AI
    let mut file = repo.filename("git-ai-integration-service.ts");
    file.assert_lines_and_blame(crate::lines![
        "/**".human(),
        " * Service for integrating git-ai hooks into the hook system.".human(),
        " */".human(),
        "export class GitAiIntegrationService {".human(),
        "  private readonly commandPath: string;".human(),
        // "  private registered = false;".human(),
        // "".human(),
        "  constructor(_config: Config, enabled: boolean = true) {".human(),
        // "    this.enabled = enabled;".human(),
        "    this.commandPath = 'git-ai';".human(),
        "  }".human(),
        "}".human(),
    ]);
}

/// Regression test: AI generates a full new file, then human deletes everything and
/// rewrites. The commit should report 100% human, not 100% AI.
///
/// The bug: when the human checkpoint has empty `line_attributions` but non-empty
/// byte-range `attributions` (all human), the fallback conversion in
/// `from_just_working_log` strips human lines (by design) producing an empty vec.
/// The empty result causes the code to `continue` without clearing the stale AI
/// attributions from the earlier checkpoint, so the commit is incorrectly tagged as AI.
#[test]
fn test_ai_generated_file_then_human_full_rewrite() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("jokes-cli.ts");

    let ai_content = "import * as readline from 'readline';\n\nconst jokes = [\n  \"Why don't scientists trust atoms?\",\n  \"An impasta!\"\n];";
    repo.git_ai(&["checkpoint", "human", "jokes-cli.ts"])
        .unwrap();
    fs::write(&file_path, ai_content).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "jokes-cli.ts"])
        .unwrap();

    let human_content = "console.log('hello world');\nconsole.log('goodbye');";
    repo.human_edit("jokes-cli.ts", human_content);

    repo.stage_all_and_commit("human rewrite").unwrap();

    let mut file = repo.filename("jokes-cli.ts");
    file.assert_lines_and_blame(crate::lines![
        "console.log('hello world');".human(),
        "console.log('goodbye');".human(),
    ]);
}

/// Regression test: one stale checkpoint entry with character attribution but no
/// line attribution must not abort note generation for the whole commit.
#[test]
fn test_stale_zero_width_checkpoint_entry_does_not_abort_persisted_working_log() {
    let repo = TestRepo::new();
    let feature_path = repo.path().join("feature.rs");

    repo.human_edit("feature.rs", "fn main() {}\n");
    repo.stage_all_and_commit("base").unwrap();
    let mut file = repo.filename("feature.rs");
    file.assert_committed_lines(crate::lines!["fn main() {}".human(),]);

    fs::write(
        &feature_path,
        "fn main() {}\nfn generated_by_ai() -> bool { true }\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "feature.rs"])
        .unwrap();

    let working_log = repo.current_working_logs();
    let mut checkpoints = working_log
        .read_all_checkpoints()
        .expect("checkpoints should be readable");
    let ai_checkpoint = checkpoints
        .iter_mut()
        .find(|checkpoint| checkpoint.kind == CheckpointKind::AiAgent)
        .expect("AI checkpoint should exist");
    let feature_entry = ai_checkpoint
        .entries
        .iter()
        .find(|entry| entry.file == "feature.rs")
        .expect("feature checkpoint entry should exist")
        .clone();
    let ai_author_id = feature_entry
        .line_attributions
        .iter()
        .find(|attr| {
            attr.author_id != CheckpointKind::Human.to_str() && !attr.author_id.starts_with("h_")
        })
        .expect("feature entry should have an AI line attribution")
        .author_id
        .clone();

    ai_checkpoint.entries.push(WorkingLogEntry::new(
        "stale.rs".to_string(),
        feature_entry.blob_sha,
        vec![Attribution::new(0, 0, ai_author_id, 0)],
        Vec::new(),
    ));
    working_log
        .write_all_checkpoints(&checkpoints)
        .expect("modified checkpoints should be writable");

    repo.stage_all_and_commit("AI feature").unwrap();

    file.assert_lines_and_blame(crate::lines![
        "fn main() {}".human(),
        "fn generated_by_ai() -> bool { true }".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_simple_ai_then_human_deletion,
    test_multiple_ai_checkpoints_with_human_deletions,
    test_complex_mixed_additions_and_deletions,
    test_ai_generated_file_then_human_full_rewrite,
);
