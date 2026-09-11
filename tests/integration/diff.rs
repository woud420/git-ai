use crate::repos::diff_hostility::{
    configure_hostile_diff_settings, configure_repo_external_diff_helper,
};
use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::{NewCommit, TestRepo};
use crate::repos::write_executable_script;
use crate::test_utils::diff_json;
use git_ai::model::transcript::{AiTranscript, Message};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

/// Helper to parse diff output and extract meaningful lines
#[derive(Debug, PartialEq)]
struct DiffLine {
    prefix: String,
    content: String,
    attribution: Option<String>,
}

impl DiffLine {
    fn parse(line: &str) -> Option<Self> {
        // Skip headers and hunk markers
        if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("---")
            || line.starts_with("+++")
            || line.starts_with("@@")
            || line.is_empty()
        {
            return None;
        }

        let prefix = if line.starts_with('+') {
            "+"
        } else if line.starts_with('-') {
            "-"
        } else if line.starts_with(' ') {
            " "
        } else {
            return None;
        };

        // Extract content and attribution
        let rest = &line[1..];

        // Look for attribution markers at the end
        let attribution = if rest.contains("🤖") {
            // AI attribution: extract tool name after 🤖
            let parts: Vec<&str> = rest.split("🤖").collect();
            if parts.len() > 1 {
                Some(format!("ai:{}", parts[1].trim()))
            } else {
                Some("ai:unknown".to_string())
            }
        } else if rest.contains("👤") {
            // Human attribution: extract username after 👤
            let parts: Vec<&str> = rest.split("👤").collect();
            if parts.len() > 1 {
                Some(format!("human:{}", parts[1].trim()))
            } else {
                Some("human:unknown".to_string())
            }
        } else if rest.contains("[no-data]") {
            Some("no-data".to_string())
        } else {
            None
        };

        // Extract content (everything before attribution markers)
        let content = if attribution.is_some() {
            // Remove attribution from content
            rest.split("🤖")
                .next()
                .or_else(|| rest.split("👤").next())
                .or_else(|| rest.split("[no-data]").next())
                .unwrap_or(rest)
                .trim()
                .to_string()
        } else {
            rest.trim().to_string()
        };

        Some(DiffLine {
            prefix: prefix.to_string(),
            content,
            attribution,
        })
    }
}

/// Parse all meaningful diff lines from output
fn parse_diff_output(output: &str) -> Vec<DiffLine> {
    output.lines().filter_map(DiffLine::parse).collect()
}

/// Helper to assert a line has expected prefix, content, and attribution
fn assert_diff_line(
    line: &DiffLine,
    expected_prefix: &str,
    expected_content: &str,
    expected_attribution: Option<&str>,
) {
    assert_eq!(
        line.prefix, expected_prefix,
        "Line prefix mismatch: expected '{}', got '{}' for content '{}'",
        expected_prefix, line.prefix, line.content
    );

    assert!(
        line.content.contains(expected_content),
        "Line content mismatch: expected '{}' to contain '{}', full line: {:?}",
        line.content,
        expected_content,
        line
    );

    match (expected_attribution, &line.attribution) {
        (Some(expected), Some(actual)) => {
            assert!(
                actual.contains(expected),
                "Attribution mismatch: expected '{}' to contain '{}', full line: {:?}",
                actual,
                expected,
                line
            );
        }
        (Some(expected), None) => {
            panic!(
                "Expected attribution '{}' but found none for line: {:?}",
                expected, line
            );
        }
        (None, _) => {
            // Don't care about attribution
        }
    }
}

/// Assert exact sequence of diff lines with prefix, content, and attribution
fn assert_diff_lines_exact(lines: &[DiffLine], expected: &[(&str, &str, Option<&str>)]) {
    assert_eq!(
        lines.len(),
        expected.len(),
        "Line count mismatch: expected {} lines, got {}\nExpected: {:?}\nActual: {:?}",
        expected.len(),
        lines.len(),
        expected,
        lines
    );

    for (i, (line, (exp_prefix, exp_content, exp_attr))) in
        lines.iter().zip(expected.iter()).enumerate()
    {
        assert_eq!(
            &line.prefix, exp_prefix,
            "Line {} prefix mismatch: expected '{}', got '{}'\nFull line: {:?}",
            i, exp_prefix, line.prefix, line
        );

        assert!(
            line.content.contains(exp_content),
            "Line {} content mismatch: expected to contain '{}', got '{}'\nFull line: {:?}",
            i,
            exp_content,
            line.content,
            line
        );

        match (exp_attr, &line.attribution) {
            (Some(expected_attr), Some(actual_attr)) => {
                assert!(
                    actual_attr.contains(expected_attr),
                    "Line {} attribution mismatch: expected '{}', got '{}'\nFull line: {:?}",
                    i,
                    expected_attr,
                    actual_attr,
                    line
                );
            }
            (Some(expected_attr), None) => {
                panic!(
                    "Line {} expected attribution '{}' but found none\nFull line: {:?}",
                    i, expected_attr, line
                );
            }
            (None, Some(actual_attr)) => {
                // Expected no attribution but got one - this is OK for flexibility
                eprintln!(
                    "Warning: Line {} has unexpected attribution '{}', but not enforcing",
                    i, actual_attr
                );
            }
            (None, None) => {
                // Both None, OK
            }
        }
    }
}

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn single_prompt_id(commit: &NewCommit) -> String {
    let mut session_ids: Vec<String> = commit
        .authorship_log
        .metadata
        .sessions
        .keys()
        .cloned()
        .collect();
    session_ids.sort();
    assert_eq!(
        session_ids.len(),
        1,
        "expected exactly one session id for commit {} but got {:?}",
        commit.commit_sha,
        session_ids
    );
    session_ids[0].clone()
}

fn session_id_from_prompt(prompt_id: &str) -> Option<String> {
    if prompt_id.starts_with("s_") {
        Some(
            prompt_id
                .split("::")
                .next()
                .unwrap_or(prompt_id)
                .to_string(),
        )
    } else {
        None
    }
}

fn prompt_id_for_line_in_commit(commit: &NewCommit, file_path: &str, line: u32) -> Option<String> {
    let file_attestation = commit
        .authorship_log
        .attestations
        .iter()
        .find(|attestation| attestation.file_path == file_path)?;

    for entry in &file_attestation.entries {
        if entry.line_ranges.iter().any(|range| range.contains(line)) {
            return Some(entry.hash.clone());
        }
    }

    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JsonHunk {
    commit_sha: String,
    content_hash: String,
    hunk_kind: String,
    original_commit_sha: Option<String>,
    start_line: u32,
    end_line: u32,
    file_path: String,
    prompt_id: Option<String>,
    session_id: Option<String>,
}

impl JsonHunk {
    /// Strip trace IDs from prompt_id (convert "s_xxx::t_yyy" to "s_xxx")
    fn strip_trace_id(&self) -> Self {
        Self {
            commit_sha: self.commit_sha.clone(),
            content_hash: self.content_hash.clone(),
            hunk_kind: self.hunk_kind.clone(),
            original_commit_sha: self.original_commit_sha.clone(),
            start_line: self.start_line,
            end_line: self.end_line,
            file_path: self.file_path.clone(),
            prompt_id: self
                .prompt_id
                .as_ref()
                .map(|id| id.split("::").next().unwrap_or(id).to_string()),
            session_id: self.session_id.clone(),
        }
    }
}

fn parse_json_hunks(json: &Value, file_path: &str, hunk_kind: &str) -> Vec<JsonHunk> {
    let mut hunks: Vec<JsonHunk> = json["hunks"]
        .as_array()
        .expect("hunks should be an array")
        .iter()
        .filter(|hunk| hunk["file_path"] == file_path && hunk["hunk_kind"] == hunk_kind)
        .map(|hunk| JsonHunk {
            commit_sha: hunk["commit_sha"]
                .as_str()
                .expect("commit_sha should be a string")
                .to_string(),
            content_hash: hunk["content_hash"]
                .as_str()
                .expect("content_hash should be a string")
                .to_string(),
            hunk_kind: hunk["hunk_kind"]
                .as_str()
                .expect("hunk_kind should be a string")
                .to_string(),
            original_commit_sha: hunk["original_commit_sha"]
                .as_str()
                .map(ToString::to_string),
            start_line: hunk["start_line"]
                .as_u64()
                .expect("start_line should be a number") as u32,
            end_line: hunk["end_line"]
                .as_u64()
                .expect("end_line should be a number") as u32,
            file_path: hunk["file_path"]
                .as_str()
                .expect("file_path should be a string")
                .to_string(),
            prompt_id: hunk["prompt_id"].as_str().map(ToString::to_string),
            session_id: hunk["session_id"].as_str().map(ToString::to_string),
        })
        .collect();

    hunks.sort_by(|a, b| {
        (
            a.file_path.as_str(),
            a.hunk_kind.as_str(),
            a.start_line,
            a.end_line,
            a.content_hash.as_str(),
        )
            .cmp(&(
                b.file_path.as_str(),
                b.hunk_kind.as_str(),
                b.start_line,
                b.end_line,
                b.content_hash.as_str(),
            ))
    });
    hunks
}

fn commit_keys(json: &Value) -> BTreeSet<String> {
    json["commits"]
        .as_object()
        .expect("commits should be an object")
        .keys()
        .cloned()
        .collect()
}

fn write_lines(repo: &TestRepo, file_path: &str, lines: &[&str]) {
    let full_path = repo.path().join(file_path);
    let mut contents = lines.join("\n");
    if !contents.is_empty() {
        contents.push('\n');
    }
    fs::write(full_path, contents).expect("writing test file should succeed");
}

fn checkpoint_agent_v1(
    repo: &TestRepo,
    file_path: &str,
    tool: &str,
    model: &str,
    conversation_id: &str,
    label: &str,
) {
    let mut transcript = AiTranscript::new();
    transcript.add_message(Message::user(label.to_string(), None));
    transcript.add_message(Message::assistant(
        "Applying requested changes".to_string(),
        None,
    ));

    let hook_input = serde_json::json!({
        "type": "ai_agent",
        "repo_working_dir": repo.path().to_str().unwrap(),
        "edited_filepaths": vec![file_path],
        "transcript": transcript,
        "agent_name": tool,
        "model": model,
        "conversation_id": conversation_id,
    });
    let hook_input_str = serde_json::to_string(&hook_input).expect("hook input should serialize");

    repo.checkpoint_with_hook_input("agent-v1", &hook_input_str)
        .expect("agent-v1 checkpoint should succeed");
}

fn checkpoint_human(repo: &TestRepo) {
    repo.git_ai(&["checkpoint"])
        .expect("human checkpoint should succeed");
}

fn checkpoint_known_human(repo: &TestRepo, file_path: &str) {
    repo.git_ai(&["checkpoint", "mock_known_human", file_path])
        .expect("known human checkpoint should succeed");
}

fn commit_after_staging_all(repo: &TestRepo, message: &str) -> NewCommit {
    repo.git(&["add", "-A"]).expect("git add should succeed");
    repo.commit(message).expect("commit should succeed")
}

fn commit_with_git_og_as_author(
    repo: &TestRepo,
    file_path: &str,
    lines: &[&str],
    author_name: &str,
    author_email: &str,
    message: &str,
) -> String {
    write_lines(repo, file_path, lines);
    repo.git_og(&["add", file_path])
        .expect("git add via git_og should succeed");

    let author = format!("{} <{}>", author_name, author_email);
    repo.git_og_with_env(&["commit", "-m", message, "--author", &author], &[])
        .expect("git commit via git_og should succeed");

    repo.git_og(&["rev-parse", "HEAD"])
        .expect("git rev-parse should succeed")
        .trim()
        .to_string()
}

fn tool_model_stats(ai_lines_added: u64) -> Value {
    serde_json::json!({
        "ai_lines_added": ai_lines_added
    })
}

fn assert_stats_exact(
    commit_stats: &Value,
    expected_top_level: &Value,
    expected_breakdown: &BTreeMap<String, Value>,
) {
    assert_eq!(
        commit_stats["ai_lines_added"], expected_top_level["ai_lines_added"],
        "ai_lines_added mismatch"
    );
    assert_eq!(
        commit_stats["human_lines_added"], expected_top_level["human_lines_added"],
        "human_lines_added mismatch"
    );
    assert_eq!(
        commit_stats["unknown_lines_added"], expected_top_level["unknown_lines_added"],
        "unknown_lines_added mismatch"
    );
    assert_eq!(
        commit_stats["git_lines_added"], expected_top_level["git_lines_added"],
        "git_lines_added mismatch"
    );
    assert_eq!(
        commit_stats["git_lines_deleted"], expected_top_level["git_lines_deleted"],
        "git_lines_deleted mismatch"
    );

    let actual_breakdown = commit_stats["tool_model_breakdown"]
        .as_object()
        .expect("tool_model_breakdown should be an object");
    assert_eq!(
        actual_breakdown.len(),
        expected_breakdown.len(),
        "tool_model_breakdown entry count mismatch: actual={:?}, expected={:?}",
        actual_breakdown.keys().collect::<Vec<_>>(),
        expected_breakdown.keys().collect::<Vec<_>>()
    );

    for (key, expected_stats) in expected_breakdown {
        let actual_stats = actual_breakdown
            .get(key)
            .unwrap_or_else(|| panic!("Missing tool_model_breakdown entry for {}", key));
        assert_eq!(
            actual_stats, expected_stats,
            "tool_model_breakdown mismatch for {}",
            key
        );
    }
}

fn create_external_diff_helper_script(repo: &TestRepo, marker: &str) -> std::path::PathBuf {
    let helper_path = repo.path().join(format!("ext-env-helper-{marker}.sh"));

    write_executable_script(&helper_path, format!("#!/bin/sh\necho {marker}\nexit 0\n"))
        .expect("should write external diff helper");

    helper_path
}

mod deletion_origins;
mod deletion_segments;
mod deletion_statistics;
mod formatting;
mod hostile_config;
mod human_identity;
mod line_attribution;
mod prompt_statistics;

mod reindentation;
mod rename_statistics;

/// Regression test: AI inserts comments and a blank line into an existing AI-written file.
/// The blank line is byte-identical to existing blank lines, so imara-diff matches it as
/// Equal. Git diff treats it as inserted. Without gap-filling, it shows as [no-data].
/// Reproduces exact scenario from user bug report with calcb.py.
#[test]
fn test_diff_ai_inserted_blank_line_with_comments_attributed_to_ai() {
    let repo = TestRepo::new();

    // Step 1: AI writes the initial file (first Claude session)
    let file_path = "calcb.py";
    let initial_content = "\
import sys


def add(a: int, b: int) -> int:
    return a + b


def main():
    if len(sys.argv) != 3:
        print(\"Usage: python calcb.py <int1> <int2>\")
        sys.exit(1)
    a = int(sys.argv[1])
    b = int(sys.argv[2])
    result = add(a, b)
    print(f\"{a} + {b} = {result}\")


if __name__ == \"__main__\":
    main()
";

    let full_path = repo.path().join(file_path);
    fs::write(&full_path, initial_content).expect("write initial content");
    repo.git_ai(&["checkpoint", "mock_ai", file_path])
        .expect("checkpoint initial write");
    repo.git(&["add", file_path]).expect("git add");
    repo.commit("initial").expect("initial commit");

    // Step 2: AI adds comments and a blank line (second Claude session edit)
    let edited_content = "\
import sys

# Simple integer addition calculator
# Accepts two integers as command-line arguments


def add(a: int, b: int) -> int:
    \"\"\"Return the sum of two integers.\"\"\"
    return a + b


def main():
    # Validate that exactly two arguments are provided
    if len(sys.argv) != 3:
        print(\"Usage: python calcb.py <int1> <int2>\")
        sys.exit(1)
    a = int(sys.argv[1])
    b = int(sys.argv[2])
    result = add(a, b)
    # Display the result in a readable format
    print(f\"{a} + {b} = {result}\")


if __name__ == \"__main__\":
    main()
";

    fs::write(&full_path, edited_content).expect("write edited content");
    repo.git_ai(&["checkpoint", "mock_ai", file_path])
        .expect("checkpoint edit");
    repo.git(&["add", file_path]).expect("git add");
    let commit = repo.commit("add comments").expect("commit");

    // Step 3: verify no [no-data] lines in the diff
    let diff_output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git ai diff should succeed");

    let diff_lines = parse_diff_output(&diff_output);
    let added_lines: Vec<&DiffLine> = diff_lines.iter().filter(|l| l.prefix == "+").collect();

    assert!(
        !added_lines.is_empty(),
        "Expected added lines in diff output.\nFull diff:\n{}",
        diff_output
    );

    let no_data_lines: Vec<&&DiffLine> = added_lines
        .iter()
        .filter(|l| l.attribution.as_deref() == Some("no-data"))
        .collect();

    assert!(
        no_data_lines.is_empty(),
        "Found {} added lines with [no-data] that should be attributed to AI:\n{}\nFull diff:\n{}",
        no_data_lines.len(),
        no_data_lines
            .iter()
            .map(|l| format!("  +{} [no-data]", l.content))
            .collect::<Vec<_>>()
            .join("\n"),
        diff_output
    );
}

#[test]
fn test_diff_json_sessions_use_session_id_not_combined_id() {
    let repo = TestRepo::new();

    write_lines(&repo, "example.txt", &["base"]);
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base");

    write_lines(&repo, "example.txt", &["base", "claude line"]);
    checkpoint_agent_v1(
        &repo,
        "example.txt",
        "claude",
        "opus-4-6",
        "conv-123",
        "add line",
    );

    let commit = commit_after_staging_all(&repo, "add AI line");
    let diff = diff_json(&repo, &["diff", &commit.commit_sha, "--json"]);

    let sessions = diff["sessions"]
        .as_object()
        .expect("sessions should be an object");

    let annotations = diff["files"]["example.txt"]["annotations"]
        .as_object()
        .expect("annotations should be an object");

    let hunks = diff["hunks"].as_array().expect("hunks should be an array");

    // Bug: sessions object uses combined ID (s_xxx::t_yyy) as key
    // Expected: sessions object should use session ID (s_xxx) as key
    let session_keys: Vec<String> = sessions.keys().cloned().collect();
    assert_eq!(session_keys.len(), 1, "should have exactly one session");

    let session_key = &session_keys[0];
    assert!(
        !session_key.contains("::"),
        "session key should be session ID only (s_xxx), not combined ID (s_xxx::t_yyy). Found: {}",
        session_key
    );
    assert!(
        session_key.starts_with("s_"),
        "session key should start with s_. Found: {}",
        session_key
    );

    // Annotations should still use combined ID for line attribution
    let annotation_keys: Vec<String> = annotations.keys().cloned().collect();
    assert_eq!(
        annotation_keys.len(),
        1,
        "should have exactly one annotation"
    );
    let annotation_key = &annotation_keys[0];
    assert!(
        annotation_key.contains("::"),
        "annotation key should be combined ID (s_xxx::t_yyy). Found: {}",
        annotation_key
    );

    // Hunks should use combined ID in prompt_id field
    let addition_hunk = hunks
        .iter()
        .find(|h| h["hunk_kind"] == "addition")
        .expect("should have addition hunk");
    let prompt_id = addition_hunk["prompt_id"]
        .as_str()
        .expect("prompt_id should be string");
    assert!(
        prompt_id.contains("::"),
        "hunk prompt_id should be combined ID (s_xxx::t_yyy). Found: {}",
        prompt_id
    );

    // Session key and annotation/hunk prefix should match
    assert!(
        annotation_key.starts_with(session_key),
        "annotation key {} should start with session key {}",
        annotation_key,
        session_key
    );
    assert!(
        prompt_id.starts_with(session_key),
        "prompt_id {} should start with session key {}",
        prompt_id,
        session_key
    );
}

#[test]
fn test_diff_commit_range() {
    let repo = TestRepo::new();

    // First commit
    let mut file = repo.filename("range.txt");
    file.set_contents(crate::lines!["Line 1".human()]);
    let first = repo.stage_all_and_commit("First commit").unwrap();

    // Second commit
    file.set_contents(crate::lines!["Line 1".human(), "Line 2".ai()]);
    repo.stage_all_and_commit("Second commit").unwrap();

    // Third commit
    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".human()
    ]);
    let third = repo.stage_all_and_commit("Third commit").unwrap();

    // Run git-ai diff with range
    let range = format!("{}..{}", first.commit_sha, third.commit_sha);
    let output = repo
        .git_ai(&["diff", &range])
        .expect("git-ai diff range should succeed");

    // Verify output
    assert!(output.contains("diff --git"), "Should contain diff header");
    assert!(output.contains("range.txt"), "Should mention the file");
    assert!(
        output.contains("+Line 2") || output.contains("Line 2"),
        "Should show added line"
    );
    assert!(
        output.contains("+Line 3") || output.contains("Line 3"),
        "Should show added line"
    );
}

#[test]
fn test_diff_two_positional_revisions_uses_git_range_semantics() {
    let repo = TestRepo::new();

    // Ensure the "from" commit has a parent so the regression catches accidental from^..from behavior.
    repo.git(&["commit", "--allow-empty", "-m", "Empty initial"])
        .expect("empty commit should succeed");

    let mut file = repo.filename("range_positional.txt");
    file.set_contents(crate::lines!["BASE".human()]);
    let from = repo.stage_all_and_commit("Base commit").unwrap();

    file.set_contents(crate::lines![
        "BASE".human(),
        "AI line 1".ai(),
        "AI line 2".ai()
    ]);
    let to = repo.stage_all_and_commit("Append lines").unwrap();

    let plain_git_diff = repo
        .git_og(&["--no-pager", "diff", &from.commit_sha, &to.commit_sha])
        .expect("plain git diff should succeed");
    assert!(
        plain_git_diff.contains("+AI line 1") && plain_git_diff.contains("+AI line 2"),
        "plain git diff sanity check failed:\n{}",
        plain_git_diff
    );
    assert!(
        !plain_git_diff.contains("new file mode"),
        "plain git diff should not treat this as a new file:\n{}",
        plain_git_diff
    );

    let git_ai_diff = repo
        .git_ai(&["diff", &from.commit_sha, &to.commit_sha])
        .expect("git-ai diff should support two positional revisions");

    assert!(
        git_ai_diff.contains("+AI line 1") && git_ai_diff.contains("+AI line 2"),
        "git-ai diff should include net additions between from/to commits:\n{}",
        git_ai_diff
    );
    assert!(
        !git_ai_diff.contains("new file mode") && !git_ai_diff.contains("--- /dev/null"),
        "git-ai diff should not fallback to from^..from behavior:\n{}",
        git_ai_diff
    );
}

#[test]
fn test_diff_multiple_files() {
    let repo = TestRepo::new();

    // Initial commit
    let mut file1 = repo.filename("file1.txt");
    let mut file2 = repo.filename("file2.txt");
    file1.set_contents(crate::lines!["File 1 line 1".human()]);
    file2.set_contents(crate::lines!["File 2 line 1".human()]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // Modify both files
    file1.set_contents(crate::lines!["File 1 line 1".human(), "File 1 line 2".ai()]);
    file2.set_contents(crate::lines![
        "File 2 line 1".human(),
        "File 2 line 2".human()
    ]);
    let commit = repo.stage_all_and_commit("Modify both files").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff should succeed");

    // Should show both files
    assert!(output.contains("file1.txt"), "Should mention file1");
    assert!(output.contains("file2.txt"), "Should mention file2");

    // Should have multiple diff sections
    let diff_count = output.matches("diff --git").count();
    assert_eq!(diff_count, 2, "Should have 2 diff sections");
}

#[test]
fn test_diff_initial_commit() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut file = repo.filename("initial.txt");
    file.set_contents(crate::lines!["Initial line".ai()]);
    let commit = repo.stage_all_and_commit("Initial commit").unwrap();

    // Run diff on initial commit (should compare to empty tree)
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff on initial commit should succeed");

    // Parse and verify exact sequence
    let lines = parse_diff_output(&output);

    // Should have exactly 1 addition, no deletions
    assert_diff_lines_exact(
        &lines,
        &[
            ("+", "Initial line", Some("ai")), // Only addition with AI attribution
        ],
    );
}

#[test]
fn test_diff_with_head_ref() {
    let repo = TestRepo::new();

    // Initial commit
    let mut file = repo.filename("head_test.txt");
    file.set_contents(crate::lines!["Line 1".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Second commit
    file.set_contents(crate::lines!["Line 1".human(), "Line 2".ai()]);
    repo.stage_all_and_commit("Add line").unwrap();

    // Run diff using HEAD
    let output = repo
        .git_ai(&["diff", "HEAD"])
        .expect("git-ai diff HEAD should succeed");

    // Should work with HEAD reference
    assert!(output.contains("diff --git"), "Should contain diff header");
    assert!(output.contains("head_test.txt"), "Should mention the file");
}

#[test]
fn test_diff_json_include_stats_rejects_commit_ranges() {
    let repo = TestRepo::new();

    let mut file = repo.filename("range_stats.txt");
    file.set_contents(crate::lines!["line 1".human()]);
    let first = repo.stage_all_and_commit("Commit 1").unwrap();

    file.set_contents(crate::lines!["line 1".human(), "line 2".ai()]);
    let second = repo.stage_all_and_commit("Commit 2").unwrap();

    let range = format!("{}..{}", first.commit_sha, second.commit_sha);
    let result = repo.git_ai(&["diff", &range, "--json", "--include-stats"]);
    assert!(
        result.is_err(),
        "--include-stats should be rejected for commit ranges"
    );
}

#[test]
fn test_diff_range_multiple_commits() {
    let repo = TestRepo::new();

    // First commit
    let mut file = repo.filename("multi.txt");
    file.set_contents(crate::lines!["Line 1".human()]);
    let first = repo.stage_all_and_commit("First").unwrap();

    // Second commit
    file.set_contents(crate::lines!["Line 1".human(), "Line 2".ai()]);
    repo.stage_all_and_commit("Second").unwrap();

    // Third commit
    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".human()
    ]);
    repo.stage_all_and_commit("Third").unwrap();

    // Fourth commit
    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".human(),
        "Line 4".ai()
    ]);
    let fourth = repo.stage_all_and_commit("Fourth").unwrap();

    // Run diff across multiple commits
    let range = format!("{}..{}", first.commit_sha, fourth.commit_sha);
    let output = repo
        .git_ai(&["diff", &range])
        .expect("git-ai diff multi-commit range should succeed");

    // Should show cumulative changes
    assert!(output.contains("+Line 2"), "Should show Line 2 addition");
    assert!(output.contains("+Line 3"), "Should show Line 3 addition");
    assert!(output.contains("+Line 4"), "Should show Line 4 addition");

    // Should have attribution markers
    assert!(
        output.contains("🤖") || output.contains("👤"),
        "Should have attribution markers"
    );
}

crate::reuse_tests_in_worktree!(
    test_diff_ai_inserted_blank_line_with_comments_attributed_to_ai,
    test_diff_json_sessions_use_session_id_not_combined_id,
    test_diff_commit_range,
    test_diff_multiple_files,
    test_diff_initial_commit,
    test_diff_with_head_ref,
    test_diff_json_include_stats_rejects_commit_ranges,
    test_diff_range_multiple_commits,
);
