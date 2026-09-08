use super::{ExpectedLineExt, TestRepo, commit_as_agent, commit_as_human};

// =============================================================================
// Basic agent detection: each known agent email
// =============================================================================

#[test]
fn test_agent_blame_cursor_email() {
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "test.rs",
        "fn main() {\n    println!(\"hello\");\n}\n",
        "Cursor Agent",
        "cursoragent@cursor.com",
        "feat: add main function",
    );

    let output = repo.git_ai(&["blame", "test.rs"]).unwrap();

    // All lines should show "cursor" as the author (AI agent)
    for line in output.lines() {
        assert!(
            line.contains("cursor"),
            "Expected 'cursor' in blame line, got: {}",
            line
        );
    }
}

#[test]
fn test_agent_blame_copilot_email() {
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "test.rs",
        "const x = 42;\n",
        "Copilot",
        "198982749+Copilot@users.noreply.github.com",
        "feat: add constant",
    );

    let output = repo.git_ai(&["blame", "test.rs"]).unwrap();

    // Should show github-copilot (which contains "copilot")
    for line in output.lines() {
        assert!(
            line.to_lowercase().contains("copilot"),
            "Expected 'copilot' in blame line, got: {}",
            line
        );
    }
}

#[test]
fn test_agent_blame_devin_email() {
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "test.rs",
        "use std::io;\n",
        "devin-ai-integration[bot]",
        "158243242+devin-ai-integration[bot]@users.noreply.github.com",
        "feat: add import",
    );

    let output = repo.git_ai(&["blame", "test.rs"]).unwrap();

    for line in output.lines() {
        assert!(
            line.contains("devin"),
            "Expected 'devin' in blame line, got: {}",
            line
        );
    }
}

#[test]
fn test_agent_blame_claude_email() {
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "test.py",
        "def hello():\n    return 'world'\n",
        "Claude",
        "noreply@anthropic.com",
        "feat: add hello",
    );

    let output = repo.git_ai(&["blame", "test.py"]).unwrap();

    for line in output.lines() {
        assert!(
            line.contains("claude"),
            "Expected 'claude' in blame line, got: {}",
            line
        );
    }
}

#[test]
fn test_agent_blame_codex_email() {
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "test.js",
        "console.log('hello');\n",
        "Codex",
        "noreply@openai.com",
        "feat: add log",
    );

    let output = repo.git_ai(&["blame", "test.js"]).unwrap();

    for line in output.lines() {
        assert!(
            line.contains("codex"),
            "Expected 'codex' in blame line, got: {}",
            line
        );
    }
}

// =============================================================================
// Using assert_lines_and_blame with the TestFile harness
// =============================================================================

#[test]
fn test_agent_blame_assert_lines_cursor() {
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "cursor_file.rs",
        "line1\nline2\nline3\n",
        "Cursor Agent",
        "cursoragent@cursor.com",
        "cursor commit",
    );

    let mut file = crate::repos::test_file::TestFile::new_with_filename(
        repo.path().join("cursor_file.rs"),
        vec![],
        &repo,
    );

    file.assert_lines_and_blame(vec!["line1".ai(), "line2".ai(), "line3".ai()]);
}

#[test]
fn test_agent_blame_assert_lines_mixed_human_agent() {
    let repo = TestRepo::new();

    // Human writes first line
    commit_as_human(&repo, "mixed2.rs", "human line\n", "human commit");

    // Cursor adds two more lines
    commit_as_agent(
        &repo,
        "mixed2.rs",
        "human line\nagent line 1\nagent line 2\n",
        "Cursor Agent",
        "cursoragent@cursor.com",
        "cursor commit",
    );

    let mut file = crate::repos::test_file::TestFile::new_with_filename(
        repo.path().join("mixed2.rs"),
        vec![],
        &repo,
    );

    file.assert_lines_and_blame(vec![
        "human line".human(),
        "agent line 1".ai(),
        "agent line 2".ai(),
    ]);
}

// =============================================================================
// Human commits should NOT be affected by agent detection
// =============================================================================

#[test]
fn test_agent_blame_human_email_not_detected_as_agent() {
    let repo = TestRepo::new();

    commit_as_human(&repo, "human.rs", "line 1\nline 2\n", "human commit");

    let output = repo.git_ai(&["blame", "human.rs"]).unwrap();

    // Should show the human author, NOT any AI tool name
    for line in output.lines() {
        assert!(
            !line.contains("cursor")
                && !line.contains("claude")
                && !line.contains("codex")
                && !line.contains("devin")
                && !line.contains("copilot"),
            "Human commit should not show AI tool name, got: {}",
            line
        );
    }
}

#[test]
fn test_agent_blame_similar_email_not_detected() {
    // Emails that look similar to agent emails but aren't exact matches
    let repo = TestRepo::new();

    commit_as_agent(
        &repo,
        "test.rs",
        "line 1\n",
        "NotCursor",
        "cursor@example.com", // NOT cursoragent@cursor.com
        "not cursor commit",
    );

    let output = repo.git_ai(&["blame", "test.rs"]).unwrap();

    // Should NOT be detected as cursor agent
    for line in output.lines() {
        assert!(
            !line.contains("cursor") || line.contains("NotCursor"),
            "Similar email should not trigger agent detection, got: {}",
            line
        );
    }
}

// =============================================================================
// Agent commit with existing authorship note: note should take precedence
// =============================================================================

#[test]
fn test_agent_email_with_authorship_note_uses_note() {
    // When a commit has BOTH an agent email AND an authorship note,
    // the authorship note should take precedence (existing behavior).
    let repo = TestRepo::new();

    // Use the normal TestFile flow which creates authorship notes via checkpoints
    let mut file = repo.filename("noted.rs");
    file.set_contents(crate::lines!["ai line 1".ai(), "human line 1".human(),]);
    repo.stage_all_and_commit("commit with note").unwrap();

    // Verify blame uses the authorship note (mock_ai) not any agent email detection
    let output = repo.git_ai(&["blame", "noted.rs"]).unwrap();
    assert!(
        output.contains("mock_ai"),
        "Should use authorship note tool name (mock_ai), got:\n{}",
        output
    );
}
