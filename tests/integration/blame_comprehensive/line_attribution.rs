use super::*;

// =============================================================================
// Happy Path Tests - Successful blame operations with AI authorship
// =============================================================================

#[test]
fn test_blame_success_basic_file() {
    // Happy path: Basic blame on a file with mixed human/AI authorship
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Human line 1".human(),
        "AI line 1".ai(),
        "Human line 2".human(),
        "AI line 2".ai()
    ]);

    repo.stage_all_and_commit("Mixed authorship").unwrap();

    let output = repo.git_ai(&["blame", "test.txt"]).unwrap();

    // Verify output contains all lines
    assert!(output.contains("Human line 1"));
    assert!(output.contains("AI line 1"));
    assert!(output.contains("Human line 2"));
    assert!(output.contains("AI line 2"));

    // Verify output shows AI tool name for AI lines
    assert!(output.contains("mock_ai"));
}

#[test]
fn test_blame_success_only_human_lines() {
    // Happy path: File with only human-authored lines
    let repo = TestRepo::new();
    let mut file = repo.filename("human.txt");

    file.set_contents(crate::lines![
        "Human line 1".human(),
        "Human line 2".human()
    ]);

    repo.stage_all_and_commit("All human").unwrap();

    let output = repo.git_ai(&["blame", "human.txt"]).unwrap();

    assert!(output.contains("Human line 1"));
    assert!(output.contains("Human line 2"));
    assert!(output.contains("Test User"));
    assert!(!output.contains("mock_ai"));
}

#[test]
fn test_blame_success_only_ai_lines() {
    // Happy path: File with only AI-authored lines
    let repo = TestRepo::new();
    let mut file = repo.filename("ai.txt");

    file.set_contents(crate::lines!["AI line 1".ai(), "AI line 2".ai()]);

    repo.stage_all_and_commit("All AI").unwrap();

    let output = repo.git_ai(&["blame", "ai.txt"]).unwrap();

    assert!(output.contains("AI line 1"));
    assert!(output.contains("AI line 2"));
    assert!(output.contains("mock_ai"));
}

#[test]
fn test_blame_success_with_newest_commit() {
    // Happy path: Blame at a specific commit using the API directly
    let repo = TestRepo::new();
    let mut file = repo.filename("versioned.txt");

    file.set_contents(crate::lines!["Version 1"]);
    let commit1 = repo.stage_all_and_commit("First version").unwrap();

    file.set_contents(crate::lines!["Version 2"]);
    repo.stage_all_and_commit("Second version").unwrap();

    // Use the Repository API to test newest_commit option
    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");

    let options = GitAiBlameOptions {
        newest_commit: Some(commit1.commit_sha.clone()),
        no_output: true,
        ..Default::default()
    };

    let (line_authors, _) = gitai_repo.blame("versioned.txt", &options).unwrap();

    // At commit1, should only see the first version
    assert!(!line_authors.is_empty());
}

#[test]
fn test_blame_success_json_format() {
    // Happy path: JSON output format with AI authorship
    let repo = TestRepo::new();
    let mut file = repo.filename("json_test.txt");

    file.set_contents(crate::lines!["Human line".human(), "AI line".ai()]);

    repo.stage_all_and_commit("JSON test").unwrap();

    let output = repo.git_ai(&["blame", "--json", "json_test.txt"]).unwrap();

    // Verify JSON structure
    assert!(output.contains("\"lines\""));
    assert!(output.contains("\"prompts\""));

    // Parse JSON to verify structure
    let json: serde_json::Value =
        serde_json::from_str(&output).expect("Output should be valid JSON");

    assert!(json["lines"].is_object());
    assert!(json["prompts"].is_object());
}

// =============================================================================
// AI Authorship Tests - Hunk splitting, human author attribution
// =============================================================================

#[test]
fn test_blame_ai_authorship_hunk_splitting() {
    // AI authorship: Hunks should split when different humans author lines
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2", "Line 3"]);

    let commit_sha = repo.stage_all_and_commit("Initial").unwrap().commit_sha;

    // Create authorship log with different human authors for different lines
    let mut authorship_log = AuthorshipLog::new();
    authorship_log.metadata.base_commit_sha = commit_sha.clone();

    // Prompt 1 for line 1
    let prompt_hash_1 = "prompt1".to_string();
    authorship_log.metadata.prompts.insert(
        prompt_hash_1.clone(),
        PromptRecord {
            agent_id: AgentId {
                tool: "cursor".to_string(),
                id: "session1".to_string(),
                model: "claude-3-sonnet".to_string(),
            },
            human_author: Some("Alice <alice@example.com>".to_string()),
            total_additions: 1,
            total_deletions: 0,
            accepted_lines: 1,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    // Prompt 2 for line 2
    let prompt_hash_2 = "prompt2".to_string();
    authorship_log.metadata.prompts.insert(
        prompt_hash_2.clone(),
        PromptRecord {
            agent_id: AgentId {
                tool: "cursor".to_string(),
                id: "session2".to_string(),
                model: "claude-3-sonnet".to_string(),
            },
            human_author: Some("Bob <bob@example.com>".to_string()),
            total_additions: 1,
            total_deletions: 0,
            accepted_lines: 1,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    let mut file_attestation = FileAttestation::new("test.txt".to_string());
    file_attestation.add_entry(AttestationEntry::new(
        prompt_hash_1,
        vec![LineRange::Single(1)],
    ));
    file_attestation.add_entry(AttestationEntry::new(
        prompt_hash_2,
        vec![LineRange::Single(2)],
    ));
    authorship_log.attestations.push(file_attestation);

    let note_content = authorship_log.serialize_to_string().unwrap();
    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");
    write_note(&gitai_repo, &commit_sha, &note_content).unwrap();

    // Get hunks with split_hunks_by_ai_author enabled
    let options = GitAiBlameOptions {
        split_hunks_by_ai_author: true,
        ..Default::default()
    };

    let hunks = gitai_repo.blame_hunks("test.txt", 1, 3, &options).unwrap();

    // Should have separate hunks for different human authors
    let ai_authors: Vec<_> = hunks.iter().map(|h| h.ai_human_author.clone()).collect();

    assert!(ai_authors.contains(&Some("Alice <alice@example.com>".to_string())));
    assert!(ai_authors.contains(&Some("Bob <bob@example.com>".to_string())));
}

#[test]
fn test_blame_ai_authorship_no_splitting() {
    // AI authorship: When split_hunks_by_ai_author is false, don't split
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2"]);
    let commit_sha = repo.stage_all_and_commit("Initial").unwrap().commit_sha;

    let mut authorship_log = AuthorshipLog::new();
    authorship_log.metadata.base_commit_sha = commit_sha.clone();

    let prompt_hash = "prompt1".to_string();
    authorship_log.metadata.prompts.insert(
        prompt_hash.clone(),
        PromptRecord {
            agent_id: AgentId {
                tool: "cursor".to_string(),
                id: "session1".to_string(),
                model: "claude-3-sonnet".to_string(),
            },
            human_author: Some("Alice <alice@example.com>".to_string()),
            total_additions: 2,
            total_deletions: 0,
            accepted_lines: 2,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    let mut file_attestation = FileAttestation::new("test.txt".to_string());
    file_attestation.add_entry(AttestationEntry::new(
        prompt_hash,
        vec![LineRange::Range(1, 2)],
    ));
    authorship_log.attestations.push(file_attestation);

    let note_content = authorship_log.serialize_to_string().unwrap();
    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");
    write_note(&gitai_repo, &commit_sha, &note_content).unwrap();

    let options = GitAiBlameOptions {
        split_hunks_by_ai_author: false,
        ..Default::default()
    };

    let hunks = gitai_repo.blame_hunks("test.txt", 1, 2, &options).unwrap();

    // Should have single hunk covering both lines
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].range, (1, 2));
}

#[test]
fn test_blame_ai_authorship_return_human_as_human() {
    // AI authorship: return_human_authors_as_human flag
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Human line".human()]);
    repo.stage_all_and_commit("Test").unwrap();

    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");

    let options = GitAiBlameOptions {
        return_human_authors_as_human: true,
        no_output: true,
        ..Default::default()
    };

    let (line_authors, _) = gitai_repo.blame("test.txt", &options).unwrap();

    // Human lines should be marked as "Human" (case-insensitive check)
    let author = line_authors.get(&1).unwrap();
    assert!(
        author.eq_ignore_ascii_case("human"),
        "Expected 'Human' but got '{}'",
        author
    );
}

crate::reuse_tests_in_worktree!(
    test_blame_success_basic_file,
    test_blame_success_only_human_lines,
    test_blame_success_only_ai_lines,
    test_blame_success_with_newest_commit,
    test_blame_success_json_format,
    test_blame_ai_authorship_hunk_splitting,
    test_blame_ai_authorship_no_splitting,
    test_blame_ai_authorship_return_human_as_human,
);
