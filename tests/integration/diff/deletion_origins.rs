use super::*;

#[test]
fn test_diff_blame_deletions_terminal_annotations() {
    let repo = TestRepo::new();

    let mut file = repo.filename("deletion_terminal.txt");
    file.set_contents(crate::lines![
        "keep".human(),
        "delete ai".ai(),
        "tail".human()
    ]);
    repo.stage_all_and_commit("Seed AI deletion line").unwrap();

    file.set_contents(crate::lines!["keep".human(), "tail".human()]);
    let deletion_commit = repo.stage_all_and_commit("Delete AI line").unwrap();

    let without_flag = repo
        .git_ai(&["diff", &deletion_commit.commit_sha])
        .expect("diff without --blame-deletions should succeed");
    let without_line = parse_diff_output(&without_flag)
        .into_iter()
        .find(|line| line.prefix == "-" && line.content.contains("delete ai"))
        .expect("expected deleted line in diff output");
    let without_has_ai = without_line
        .attribution
        .as_ref()
        .map(|value| value.contains("ai"))
        .unwrap_or(false);
    assert!(
        !without_has_ai,
        "deleted line should not have AI attribution without --blame-deletions"
    );

    let with_flag = repo
        .git_ai(&["diff", &deletion_commit.commit_sha, "--blame-deletions"])
        .expect("diff with --blame-deletions should succeed");
    let with_line = parse_diff_output(&with_flag)
        .into_iter()
        .find(|line| line.prefix == "-" && line.content.contains("delete ai"))
        .expect("expected deleted line in diff output");
    let with_has_ai = with_line
        .attribution
        .as_ref()
        .map(|value| value.contains("ai"))
        .unwrap_or(false);
    assert!(
        with_has_ai,
        "deleted line should include AI attribution with --blame-deletions, got: {:?}",
        with_line
    );
}

#[test]
fn test_diff_blame_deletions_since_accepts_git_date_specs() {
    let repo = TestRepo::new();

    let mut file = repo.filename("deletion_since.txt");
    file.set_contents(crate::lines![
        "keep".human(),
        "remove me".ai(),
        "tail".human()
    ]);
    repo.stage_all_and_commit("Seed AI line").unwrap();

    file.set_contents(crate::lines!["keep".human(), "tail".human()]);
    let deletion_commit = repo.stage_all_and_commit("Delete AI line").unwrap();

    let json_output = repo
        .git_ai(&[
            "diff",
            &deletion_commit.commit_sha,
            "--json",
            "--blame-deletions",
            "--blame-deletions-since",
            "2999-01-01",
        ])
        .expect("diff --json with blame-deletions-since should succeed");
    let json: Value = serde_json::from_str(&json_output).expect("diff JSON should parse");

    let deletion_hunks: Vec<&Value> = json["hunks"]
        .as_array()
        .expect("hunks should be array")
        .iter()
        .filter(|hunk| hunk["file_path"] == "deletion_since.txt" && hunk["hunk_kind"] == "deletion")
        .collect();
    assert!(!deletion_hunks.is_empty(), "expected deletion hunks");
    let relative_date_output = repo
        .git_ai(&[
            "diff",
            &deletion_commit.commit_sha,
            "--json",
            "--blame-deletions",
            "--blame-deletions-since",
            "2 weeks ago",
        ])
        .expect("diff with relative blame-deletions-since date should succeed");
    let relative_json: Value =
        serde_json::from_str(&relative_date_output).expect("relative date JSON should parse");
    let relative_deletion_hunks = relative_json["hunks"]
        .as_array()
        .expect("hunks should be array")
        .iter()
        .filter(|hunk| hunk["file_path"] == "deletion_since.txt" && hunk["hunk_kind"] == "deletion")
        .count();
    assert!(
        relative_deletion_hunks > 0,
        "relative date should still produce deletion hunks"
    );
}

#[test]
fn test_diff_json_deleted_hunks_line_level_exact_mapping() {
    let repo = TestRepo::new();

    let mut file = repo.filename("deletion_exact.txt");
    file.set_contents(crate::lines![
        "keep head".human(),
        "AI drop one".ai(),
        "human drop".human(),
        "AI drop two".ai(),
        "keep tail".human()
    ]);
    let source_commit = repo
        .stage_all_and_commit("Seed exact deletion lines")
        .unwrap();
    let source_prompt_id = single_prompt_id(&source_commit);

    file.set_contents(crate::lines!["keep head".human(), "keep tail".human()]);
    let deletion_commit = repo
        .stage_all_and_commit("Delete exact target lines")
        .unwrap();

    let json_output = repo
        .git_ai(&[
            "diff",
            &deletion_commit.commit_sha,
            "--json",
            "--blame-deletions",
        ])
        .expect("diff --json --blame-deletions should succeed");
    let json: Value = serde_json::from_str(&json_output).expect("diff JSON should parse");

    let deletion_hunks = parse_json_hunks(&json, "deletion_exact.txt", "deletion");
    let expected = vec![
        JsonHunk {
            commit_sha: deletion_commit.commit_sha.clone(),
            content_hash: sha256_hex("AI drop one"),
            hunk_kind: "deletion".to_string(),
            original_commit_sha: Some(source_commit.commit_sha.clone()),
            start_line: 2,
            end_line: 2,
            file_path: "deletion_exact.txt".to_string(),
            prompt_id: Some(source_prompt_id.clone()),
            session_id: Some(source_prompt_id.clone()),
        },
        JsonHunk {
            commit_sha: deletion_commit.commit_sha.clone(),
            content_hash: sha256_hex("human drop"),
            hunk_kind: "deletion".to_string(),
            original_commit_sha: Some(source_commit.commit_sha.clone()),
            start_line: 3,
            end_line: 3,
            file_path: "deletion_exact.txt".to_string(),
            prompt_id: None,
            session_id: None,
        },
        JsonHunk {
            commit_sha: deletion_commit.commit_sha.clone(),
            content_hash: sha256_hex("AI drop two"),
            hunk_kind: "deletion".to_string(),
            original_commit_sha: Some(source_commit.commit_sha.clone()),
            start_line: 4,
            end_line: 4,
            file_path: "deletion_exact.txt".to_string(),
            prompt_id: Some(source_prompt_id.clone()),
            session_id: Some(source_prompt_id),
        },
    ];
    // Strip trace IDs from actual hunks for comparison (sessions format includes trace IDs)
    let deletion_hunks_normalized: Vec<JsonHunk> =
        deletion_hunks.iter().map(|h| h.strip_trace_id()).collect();
    assert_eq!(deletion_hunks_normalized, expected);

    let expected_commit_keys = BTreeSet::from([
        source_commit.commit_sha.clone(),
        deletion_commit.commit_sha.clone(),
    ]);
    assert_eq!(commit_keys(&json), expected_commit_keys);

    let commits = json["commits"]
        .as_object()
        .expect("commits should be object");
    assert_eq!(
        commits[&source_commit.commit_sha]["msg"]
            .as_str()
            .expect("msg should be string"),
        "Seed exact deletion lines"
    );
    assert_eq!(
        commits[&deletion_commit.commit_sha]["msg"]
            .as_str()
            .expect("msg should be string"),
        "Delete exact target lines"
    );
}

#[test]
fn test_diff_json_deleted_hunks_exact_replacement_from_known_origin_commit() {
    let repo = TestRepo::new();
    let mut file = repo.filename("replacement_exact.txt");

    file.set_contents(crate::lines!["a".ai(), "b".ai(), "c".ai()]);
    let commit_a = repo.stage_all_and_commit("A writes abc").unwrap();
    let prompt_a = single_prompt_id(&commit_a);

    file.replace_at(0, "b".ai());
    let commit_b = repo.stage_all_and_commit("B replaces first line").unwrap();
    let prompt_b = single_prompt_id(&commit_b);

    let output = repo
        .git_ai(&["diff", &commit_b.commit_sha, "--json", "--blame-deletions"])
        .expect("diff --json --blame-deletions should succeed");
    let json: Value = serde_json::from_str(&output).expect("diff JSON should parse");

    let deletion_hunks = parse_json_hunks(&json, "replacement_exact.txt", "deletion");
    let addition_hunks = parse_json_hunks(&json, "replacement_exact.txt", "addition");

    // Strip trace IDs for comparison (sessions format includes trace IDs)
    let deletion_hunks_normalized: Vec<JsonHunk> =
        deletion_hunks.iter().map(|h| h.strip_trace_id()).collect();
    let addition_hunks_normalized: Vec<JsonHunk> =
        addition_hunks.iter().map(|h| h.strip_trace_id()).collect();

    assert_eq!(
        deletion_hunks_normalized,
        vec![JsonHunk {
            commit_sha: commit_b.commit_sha.clone(),
            content_hash: sha256_hex("a"),
            hunk_kind: "deletion".to_string(),
            original_commit_sha: Some(commit_a.commit_sha.clone()),
            start_line: 1,
            end_line: 1,
            file_path: "replacement_exact.txt".to_string(),
            prompt_id: Some(prompt_a.clone()),
            session_id: Some(prompt_a),
        }]
    );
    assert_eq!(
        addition_hunks_normalized,
        vec![JsonHunk {
            commit_sha: commit_b.commit_sha.clone(),
            content_hash: sha256_hex("b"),
            hunk_kind: "addition".to_string(),
            original_commit_sha: None,
            start_line: 1,
            end_line: 1,
            file_path: "replacement_exact.txt".to_string(),
            prompt_id: Some(prompt_b.clone()),
            session_id: Some(prompt_b),
        }]
    );

    let expected_commit_keys =
        BTreeSet::from([commit_a.commit_sha.clone(), commit_b.commit_sha.clone()]);
    assert_eq!(commit_keys(&json), expected_commit_keys);
    let commits = json["commits"]
        .as_object()
        .expect("commits should be object");
    assert_eq!(
        commits[&commit_a.commit_sha]["msg"]
            .as_str()
            .expect("msg should be string"),
        "A writes abc"
    );
    assert_eq!(
        commits[&commit_b.commit_sha]["msg"]
            .as_str()
            .expect("msg should be string"),
        "B replaces first line"
    );
}

crate::reuse_tests_in_worktree!(
    test_diff_blame_deletions_terminal_annotations,
    test_diff_blame_deletions_since_accepts_git_date_specs,
    test_diff_json_deleted_hunks_line_level_exact_mapping,
    test_diff_json_deleted_hunks_exact_replacement_from_known_origin_commit,
);
