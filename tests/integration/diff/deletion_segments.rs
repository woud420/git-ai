use super::{
    BTreeSet, ExpectedLineExt, JsonHunk, TestRepo, Value, commit_keys, diff_json, parse_json_hunks,
    prompt_id_for_line_in_commit, session_id_from_prompt, sha256_hex,
};

#[test]
fn test_diff_json_deleted_hunks_strict_mixed_origins_and_contiguous_segments() {
    let repo = TestRepo::new();
    let mut file = repo.filename("mixed_origin_exact.txt");

    file.set_contents(crate::lines![
        "A1-ai".ai(),
        "A2-human".human(),
        "A3-ai".ai(),
        "A4-human".human(),
        "A5-ai".ai()
    ]);
    let commit_a = repo.stage_all_and_commit("A baseline mixed lines").unwrap();
    let prompt_a_line_1 = prompt_id_for_line_in_commit(&commit_a, "mixed_origin_exact.txt", 1)
        .expect("line 1 in commit A should be AI-attributed");
    let prompt_a_line_5 = prompt_id_for_line_in_commit(&commit_a, "mixed_origin_exact.txt", 5)
        .expect("line 5 in commit A should be AI-attributed");

    file.delete_range(2, 4);
    file.insert_at(2, vec!["B3-ai".ai(), "B4-ai".ai()]);
    let commit_b = repo
        .stage_all_and_commit("B rewrites middle lines")
        .unwrap();
    let prompt_b = prompt_id_for_line_in_commit(&commit_b, "mixed_origin_exact.txt", 3)
        .expect("line 3 in commit B should be AI-attributed");

    file.delete_range(2, 5);
    file.delete_at(0);
    let commit_c = repo
        .stage_all_and_commit("C deletes mixed-origin ranges")
        .unwrap();

    let output = repo
        .git_ai(&["diff", &commit_c.commit_sha, "--json", "--blame-deletions"])
        .expect("diff --json --blame-deletions should succeed");
    let json: Value = serde_json::from_str(&output).expect("diff JSON should parse");

    let deletion_hunks = parse_json_hunks(&json, "mixed_origin_exact.txt", "deletion");
    let addition_hunks = parse_json_hunks(&json, "mixed_origin_exact.txt", "addition");

    assert_eq!(
        addition_hunks,
        vec![JsonHunk {
            commit_sha: commit_c.commit_sha.clone(),
            content_hash: sha256_hex("A2-human"),
            hunk_kind: "addition".to_string(),
            original_commit_sha: None,
            start_line: 1,
            end_line: 1,
            file_path: "mixed_origin_exact.txt".to_string(),
            prompt_id: None,
            session_id: None,
        }]
    );
    assert_eq!(
        deletion_hunks,
        vec![
            JsonHunk {
                commit_sha: commit_c.commit_sha.clone(),
                content_hash: sha256_hex("A1-ai"),
                hunk_kind: "deletion".to_string(),
                original_commit_sha: Some(commit_a.commit_sha.clone()),
                start_line: 1,
                end_line: 1,
                file_path: "mixed_origin_exact.txt".to_string(),
                session_id: session_id_from_prompt(&prompt_a_line_1),
                prompt_id: Some(prompt_a_line_1),
            },
            JsonHunk {
                commit_sha: commit_c.commit_sha.clone(),
                content_hash: sha256_hex("A2-human"),
                hunk_kind: "deletion".to_string(),
                original_commit_sha: Some(commit_a.commit_sha.clone()),
                start_line: 2,
                end_line: 2,
                file_path: "mixed_origin_exact.txt".to_string(),
                prompt_id: None,
                session_id: None,
            },
            JsonHunk {
                commit_sha: commit_c.commit_sha.clone(),
                content_hash: sha256_hex("B3-ai\nB4-ai"),
                hunk_kind: "deletion".to_string(),
                original_commit_sha: Some(commit_b.commit_sha.clone()),
                start_line: 3,
                end_line: 4,
                file_path: "mixed_origin_exact.txt".to_string(),
                session_id: session_id_from_prompt(&prompt_b),
                prompt_id: Some(prompt_b),
            },
            JsonHunk {
                commit_sha: commit_c.commit_sha.clone(),
                content_hash: sha256_hex("A5-ai"),
                hunk_kind: "deletion".to_string(),
                original_commit_sha: Some(commit_a.commit_sha.clone()),
                start_line: 5,
                end_line: 5,
                file_path: "mixed_origin_exact.txt".to_string(),
                session_id: session_id_from_prompt(&prompt_a_line_5),
                prompt_id: Some(prompt_a_line_5),
            },
        ]
    );

    let expected_commit_keys = BTreeSet::from([
        commit_a.commit_sha.clone(),
        commit_b.commit_sha.clone(),
        commit_c.commit_sha.clone(),
    ]);
    assert_eq!(commit_keys(&json), expected_commit_keys);
    let commits = json["commits"]
        .as_object()
        .expect("commits should be object");
    assert_eq!(
        commits[&commit_a.commit_sha]["msg"]
            .as_str()
            .expect("msg should be string"),
        "A baseline mixed lines"
    );
    assert_eq!(
        commits[&commit_b.commit_sha]["msg"]
            .as_str()
            .expect("msg should be string"),
        "B rewrites middle lines"
    );
    assert_eq!(
        commits[&commit_c.commit_sha]["msg"]
            .as_str()
            .expect("msg should be string"),
        "C deletes mixed-origin ranges"
    );
}

#[test]
fn test_diff_json_deleted_hunks_same_content_but_different_origins() {
    let repo = TestRepo::new();
    let mut file = repo.filename("duplicate_content_exact.txt");

    file.set_contents(crate::lines![
        "top".human(),
        "dup".ai(),
        "middle".human(),
        "tail".human()
    ]);
    let commit_a = repo.stage_all_and_commit("A creates first dup").unwrap();
    let prompt_a = prompt_id_for_line_in_commit(&commit_a, "duplicate_content_exact.txt", 2)
        .expect("line 2 in commit A should be AI-attributed");

    file.insert_at(3, vec!["dup".ai()]);
    let commit_b = repo.stage_all_and_commit("B adds second dup").unwrap();
    let prompt_b = prompt_id_for_line_in_commit(&commit_b, "duplicate_content_exact.txt", 4)
        .expect("line 4 in commit B should be AI-attributed");

    file.delete_at(3);
    file.delete_at(1);
    let commit_c = repo
        .stage_all_and_commit("C deletes both dup lines")
        .unwrap();

    let output = repo
        .git_ai(&["diff", &commit_c.commit_sha, "--json", "--blame-deletions"])
        .expect("diff --json --blame-deletions should succeed");
    let json: Value = serde_json::from_str(&output).expect("diff JSON should parse");

    let deletion_hunks = parse_json_hunks(&json, "duplicate_content_exact.txt", "deletion");
    assert_eq!(
        deletion_hunks,
        vec![
            JsonHunk {
                commit_sha: commit_c.commit_sha.clone(),
                content_hash: sha256_hex("dup"),
                hunk_kind: "deletion".to_string(),
                original_commit_sha: Some(commit_a.commit_sha.clone()),
                start_line: 2,
                end_line: 2,
                file_path: "duplicate_content_exact.txt".to_string(),
                session_id: session_id_from_prompt(&prompt_a),
                prompt_id: Some(prompt_a),
            },
            JsonHunk {
                commit_sha: commit_c.commit_sha.clone(),
                content_hash: sha256_hex("dup"),
                hunk_kind: "deletion".to_string(),
                original_commit_sha: Some(commit_b.commit_sha.clone()),
                start_line: 4,
                end_line: 4,
                file_path: "duplicate_content_exact.txt".to_string(),
                session_id: session_id_from_prompt(&prompt_b),
                prompt_id: Some(prompt_b),
            },
        ]
    );

    let expected_commit_keys = BTreeSet::from([
        commit_a.commit_sha.clone(),
        commit_b.commit_sha.clone(),
        commit_c.commit_sha.clone(),
    ]);
    assert_eq!(commit_keys(&json), expected_commit_keys);
}

#[test]
fn test_diff_json_commit_author_is_full_ident() {
    let repo = TestRepo::new();
    let mut file = repo.filename("author_ident.txt");
    file.set_contents(crate::lines!["base".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    file.set_contents(crate::lines!["base".human(), "AI line".ai()]);
    let commit = repo.stage_all_and_commit("Add AI line").unwrap();

    let json = diff_json(&repo, &["diff", &commit.commit_sha, "--json"]);
    let author = json["commits"][&commit.commit_sha]["author"]
        .as_str()
        .expect("commit author should be a string");
    assert_eq!(author, "Test User <test@example.com>");
}

crate::reuse_tests_in_worktree!(
    test_diff_json_deleted_hunks_strict_mixed_origins_and_contiguous_segments,
    test_diff_json_deleted_hunks_same_content_but_different_origins,
    test_diff_json_commit_author_is_full_ident,
);
