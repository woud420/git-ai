use super::{
    DiffLine, TestRepo, checkpoint_agent_v1, checkpoint_human, commit_after_staging_all, diff_json,
    fs, parse_diff_output, write_lines,
};

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

crate::reuse_tests_in_worktree!(
    test_diff_ai_inserted_blank_line_with_comments_attributed_to_ai,
    test_diff_json_sessions_use_session_id_not_combined_id,
);
