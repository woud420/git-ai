use super::*;

#[test]
fn test_diff_json_rename_only_has_no_hunks_and_zero_stats() {
    let repo = TestRepo::new();

    write_lines(&repo, "rename_old.txt", &["line-1", "line-2"]);
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base");

    repo.git(&["mv", "rename_old.txt", "rename_new.txt"])
        .expect("git mv should succeed");
    checkpoint_human(&repo);
    let commit = commit_after_staging_all(&repo, "rename only");

    let diff = diff_json(
        &repo,
        &["diff", &commit.commit_sha, "--json", "--include-stats"],
    );

    let hunks = diff["hunks"].as_array().expect("hunks should be an array");
    assert!(
        hunks.is_empty(),
        "rename-only commit should not produce add/delete hunks"
    );

    let commit_stats = diff
        .get("commit_stats")
        .expect("commit_stats should be present");
    let expected_top_level = serde_json::json!({
        "ai_lines_added": 0,
        "human_lines_added": 0,
        "unknown_lines_added": 0,
        "git_lines_added": 0,
        "git_lines_deleted": 0
    });
    let expected_breakdown: BTreeMap<String, Value> = BTreeMap::new();
    assert_stats_exact(commit_stats, &expected_top_level, &expected_breakdown);

    let files = diff["files"].as_object().expect("files should be object");
    assert!(
        !files.is_empty(),
        "rename-only commit should still include a file diff section"
    );
    assert!(
        files.values().any(|file| {
            let text = file["diff"].as_str().unwrap_or("");
            text.contains("rename from rename_old.txt") && text.contains("rename to rename_new.txt")
        }),
        "rename-only diff should include rename metadata"
    );
}

#[test]
fn test_diff_json_rename_with_ai_edit_exact_stats() {
    let repo = TestRepo::new();

    write_lines(&repo, "rename_edit_old.txt", &["base-1", "base-2"]);
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base");

    repo.git(&["mv", "rename_edit_old.txt", "rename_edit_new.txt"])
        .expect("git mv should succeed");
    write_lines(
        &repo,
        "rename_edit_new.txt",
        &["base-1", "ai-line-2", "ai-line-3"],
    );
    checkpoint_agent_v1(
        &repo,
        "rename_edit_new.txt",
        "cursor",
        "gpt-4o",
        "rename-edit-conv",
        "rename and edit",
    );
    let commit = commit_after_staging_all(&repo, "rename with ai edit");

    let diff = diff_json(
        &repo,
        &["diff", &commit.commit_sha, "--json", "--include-stats"],
    );
    let commit_stats = diff
        .get("commit_stats")
        .expect("commit_stats should be present");

    // Math ledger:
    // old: [base-1, base-2]
    // new: [base-1, ai-line-2, ai-line-3]
    // => landed +2, -1 (all AI-attributed additions)
    let expected_top_level = serde_json::json!({
        "ai_lines_added": 2,
        "human_lines_added": 0,
        "unknown_lines_added": 0,
        "git_lines_added": 2,
        "git_lines_deleted": 1
    });
    let expected_breakdown = BTreeMap::from([("cursor::gpt-4o".to_string(), tool_model_stats(2))]);
    assert_stats_exact(commit_stats, &expected_top_level, &expected_breakdown);

    let files = diff["files"].as_object().expect("files should be object");
    assert!(
        files.values().any(|file| {
            let text = file["diff"].as_str().unwrap_or("");
            text.contains("rename from rename_edit_old.txt")
                && text.contains("rename to rename_edit_new.txt")
        }),
        "rename+edit diff should include rename metadata"
    );
}

#[test]
fn test_diff_json_blame_deletions_rename_with_edit_uses_old_path() {
    let repo = TestRepo::new();

    let mut old_file = repo.filename("rename_blame_old.txt");
    old_file.set_contents(crate::lines![
        "keep".human(),
        "drop-ai".ai(),
        "tail".human()
    ]);
    let base_commit = repo.stage_all_and_commit("base with ai line").unwrap();
    let old_line_prompt = prompt_id_for_line_in_commit(&base_commit, "rename_blame_old.txt", 2)
        .expect("line 2 in base commit should be AI-attributed");

    repo.git(&["mv", "rename_blame_old.txt", "rename_blame_new.txt"])
        .expect("git mv should succeed");
    let mut new_file = repo.filename("rename_blame_new.txt");
    new_file.set_contents(crate::lines!["keep".human(), "tail".human()]);
    let rename_commit = repo
        .stage_all_and_commit("rename and edit removing ai line")
        .unwrap();

    let output = repo
        .git_ai(&[
            "diff",
            &rename_commit.commit_sha,
            "--json",
            "--blame-deletions",
        ])
        .expect("diff --json --blame-deletions should succeed");
    let json: Value = serde_json::from_str(&output).expect("diff JSON should parse");

    let deletion_hunks = parse_json_hunks(&json, "rename_blame_new.txt", "deletion");
    assert_eq!(
        deletion_hunks,
        vec![JsonHunk {
            commit_sha: rename_commit.commit_sha.clone(),
            content_hash: sha256_hex("drop-ai"),
            hunk_kind: "deletion".to_string(),
            original_commit_sha: Some(base_commit.commit_sha.clone()),
            start_line: 2,
            end_line: 2,
            file_path: "rename_blame_new.txt".to_string(),
            session_id: session_id_from_prompt(&old_line_prompt),
            prompt_id: Some(old_line_prompt),
        }],
        "deletion blame should resolve against the old path after rename+edit"
    );

    let expected_commit_keys = BTreeSet::from([
        base_commit.commit_sha.clone(),
        rename_commit.commit_sha.clone(),
    ]);
    assert_eq!(commit_keys(&json), expected_commit_keys);
}

crate::reuse_tests_in_worktree!(
    test_diff_json_rename_only_has_no_hunks_and_zero_stats,
    test_diff_json_rename_with_ai_edit_exact_stats,
    test_diff_json_blame_deletions_rename_with_edit_uses_old_path,
);
