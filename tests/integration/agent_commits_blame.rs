//! Tests for agent commit detection in blame.
//!
//! These tests verify that commits made by known AI agents (identified by
//! their author email) are correctly attributed as AI-authored in blame output,
//! even when no explicit authorship note exists.
//!
//! TDD: These tests define the expected behavior BEFORE implementation.
//! They should fail initially and pass once agent commit detection is
//! integrated into overlay_ai_authorship.

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;

/// Extract the JSON object from git-ai output, stripping any trailing log/migration lines.
fn extract_json(output: &str) -> &str {
    // Find the outermost JSON object: first '{' to its matching '}'
    let start = match output.find('{') {
        Some(i) => i,
        None => return output,
    };
    let mut depth = 0;
    let mut end = start;
    for (i, ch) in output[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    &output[start..end]
}

// =============================================================================
// Helper: Create a commit with a specific author email using git_og (no hooks)
// =============================================================================

/// Write a file and commit it with a specific author identity, bypassing git-ai hooks.
/// This creates a commit with NO authorship note, simulating an agent commit.
fn commit_as_agent(
    repo: &TestRepo,
    filename: &str,
    contents: &str,
    author_name: &str,
    author_email: &str,
    message: &str,
) -> String {
    let file_path = repo.path().join(filename);
    // Create parent dirs if needed
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&file_path, contents).unwrap();

    repo.git_og(&["add", filename]).unwrap();

    let author_arg = format!("{} <{}>", author_name, author_email);
    repo.git_og_with_env(&["commit", "-m", message, "--author", &author_arg], &[])
        .unwrap();

    // Return the commit SHA
    repo.git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

/// Write a file and commit it as a regular human user (bypassing hooks, no authorship note).
fn commit_as_human(repo: &TestRepo, filename: &str, contents: &str, message: &str) -> String {
    commit_as_agent(
        repo,
        filename,
        contents,
        "Human Developer",
        "human@example.com",
        message,
    )
}

mod agent_identity;
mod mixed_authorship;
mod output_formats;
