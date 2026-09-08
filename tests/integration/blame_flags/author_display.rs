use super::{
    AgentId, AttestationEntry, AuthorshipLog, ExpectedLineExt, FileAttestation, GitAiBlameOptions,
    GitAiRepository, LineRange, PromptRecord, TestRepo, extract_authors, normalize_for_snapshot,
    write_note,
};

#[test]
fn test_blame_show_email() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo.git(&["blame", "-e", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "-e", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );

    // Verify both contain email addresses
    assert!(git_output.contains("@"), "Git output should contain email");
    assert!(
        git_ai_output.contains("@"),
        "Git-ai output should contain email"
    );
}

#[test]
fn test_blame_show_name() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo.git(&["blame", "-f", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "-f", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );

    // Verify both contain filename information
    assert!(
        git_output.contains("test.txt"),
        "Git output should contain filename"
    );
    assert!(
        git_ai_output.contains("test.txt"),
        "Git-ai output should contain filename"
    );
}

#[test]
fn test_blame_show_number() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo.git(&["blame", "-n", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "-n", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );
}

#[test]
fn test_blame_suppress_author() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2".ai()]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    let git_output = repo.git(&["blame", "-s", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "-s", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );

    // Verify both suppress author information (should not contain "Test User")
    assert!(
        !git_output.contains("Test User"),
        "Git output should suppress author"
    );
    assert!(
        !git_ai_output.contains("Test User"),
        "Git-ai output should suppress author"
    );
}

#[test]
fn test_blame_with_ai_authorship() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Line 1", "Line 2", "Line 3".ai(), "Line 4"]);

    repo.stage_all_and_commit("Mixed authorship commit")
        .unwrap();

    let git_output = repo.git(&["blame", "test.txt"]).unwrap();
    let git_ai_output = repo.git_ai(&["blame", "test.txt"]).unwrap();

    let git_norm = normalize_for_snapshot(&git_output);
    let git_ai_norm = normalize_for_snapshot(&git_ai_output);
    println!("\n[DEBUG] Normalized git blame output:\n{}", git_norm);
    println!("\n[DEBUG] Normalized git-ai blame output:\n{}", git_ai_norm);
    assert_eq!(
        git_norm, git_ai_norm,
        "Normalized blame outputs should match exactly"
    );

    // Extract authors from both outputs
    let git_authors = extract_authors(&git_output);
    let git_ai_authors = extract_authors(&git_ai_output);

    // Git should show the same author for all lines (the committer)
    // Git-ai should show different authors based on AI authorship
    assert_ne!(
        git_authors, git_ai_authors,
        "AI authorship should change the output"
    );

    // Verify git-ai shows AI authors where appropriate
    assert!(
        git_ai_authors
            .iter()
            .any(|a| a.contains("mock_ai") || a.contains("mock_ai")),
        "Should show AI as author. Got: {:?}",
        git_ai_authors
    );
}

#[test]
fn test_blame_ai_human_author() {
    let repo = TestRepo::new();

    let mut file = repo.filename("test.txt");

    // Create initial commit
    file.set_contents(crate::lines!["first line", "second line", "third line"]);

    let initial_sha = repo
        .stage_all_and_commit("Initial commit")
        .unwrap()
        .commit_sha;

    // Create authorship log with two prompts - one for line 1, one for line 2
    let mut authorship_log = AuthorshipLog::new();
    authorship_log.metadata.base_commit_sha = initial_sha.clone();

    // First prompt for line 1
    let prompt_hash_1 = "abc12345".to_string();
    let agent_id_1 = AgentId {
        tool: "cursor".to_string(),
        id: "session_line1".to_string(),
        model: "claude-3-sonnet".to_string(),
    };
    authorship_log.metadata.prompts.insert(
        prompt_hash_1.clone(),
        PromptRecord {
            agent_id: agent_id_1,
            human_author: Some("First <first@example.com>".to_string()),
            total_additions: 1,
            total_deletions: 0,
            accepted_lines: 1,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    // Second prompt for line 2
    let prompt_hash_2 = "xyz67890".to_string();
    let agent_id_2 = AgentId {
        tool: "cursor".to_string(),
        id: "session_line2".to_string(),
        model: "claude-3-sonnet".to_string(),
    };
    authorship_log.metadata.prompts.insert(
        prompt_hash_2.clone(),
        PromptRecord {
            agent_id: agent_id_2,
            human_author: Some("Second <second@example.com>".to_string()),
            total_additions: 1,
            total_deletions: 0,
            accepted_lines: 1,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    // Add attestations - line 1 attributed to first prompt, line 2 to second
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

    // Serialize and add the note
    let note_content = authorship_log.serialize_to_string().unwrap();
    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");
    write_note(&gitai_repo, &initial_sha, &note_content).unwrap();

    // Call blame_hunks on the file
    let options = GitAiBlameOptions::default();
    let hunks = gitai_repo
        .blame_hunks("test.txt", 1, 2, &options)
        .expect("Failed to get blame hunks");

    let ai_human_authors = hunks
        .iter()
        .map(|hunk| hunk.ai_human_author.clone())
        .collect::<Vec<_>>();

    assert_eq!(
        ai_human_authors,
        vec![
            Some("First <first@example.com>".to_string()),
            Some("Second <second@example.com>".to_string())
        ]
    );

    let args = ["blame", "--line-porcelain", "-L", "1,2", "test.txt"];
    assert_eq!(
        normalize_for_snapshot(&repo.git(&args).unwrap()),
        normalize_for_snapshot(&repo.git_ai(&args).unwrap()),
        "renderer preparation must preserve Git's unsplit porcelain hunks"
    );
}

crate::reuse_tests_in_worktree!(
    test_blame_show_email,
    test_blame_show_name,
    test_blame_show_number,
    test_blame_suppress_author,
    test_blame_with_ai_authorship,
    test_blame_ai_human_author,
);
