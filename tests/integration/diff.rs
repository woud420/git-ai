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

mod blank_lines_and_sessions;
mod deletion_origins;
mod deletion_segments;
mod deletion_statistics;
mod formatting;
mod hostile_config;
mod human_identity;
mod line_attribution;
mod prompt_statistics;
mod ranges;
mod reindentation;
mod rename_statistics;
