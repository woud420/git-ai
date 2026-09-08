use super::{
    TestRepo, Value, checkpoint_human, checkpoint_known_human, commit_after_staging_all, diff_json,
    parse_diff_output, write_lines,
};

#[test]
fn test_diff_visual_output_shows_human_author_name_not_id() {
    let repo = TestRepo::new();

    // Create a base commit
    write_lines(&repo, "human_author.txt", &["line1", "line2"]);
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base commit");

    // Add lines and create a known human checkpoint with a specific author
    write_lines(
        &repo,
        "human_author.txt",
        &["line1", "line2", "line3", "line4"],
    );
    checkpoint_known_human(&repo, "human_author.txt");
    let commit = commit_after_staging_all(&repo, "human changes");

    // Get visual diff output (not JSON)
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("diff should succeed");

    // Parse the diff to find the added lines
    let lines = parse_diff_output(&output);
    let line3 = lines
        .iter()
        .find(|l| l.prefix == "+" && l.content.contains("line3"))
        .expect("should find +line3");
    let line4 = lines
        .iter()
        .find(|l| l.prefix == "+" && l.content.contains("line4"))
        .expect("should find +line4");

    // The visual output should show the author name, not the h_-prefixed ID
    assert!(line3.attribution.is_some(), "line3 should have attribution");
    let attr = line3.attribution.as_ref().unwrap();
    assert!(
        attr.starts_with("human:"),
        "line3 attribution should be human, got: {}",
        attr
    );

    // Extract the displayed name from attribution (format is "human:<name>")
    let displayed_name = attr.strip_prefix("human:").unwrap();

    // Bug: Currently shows the h_-prefixed ID instead of the author name
    // Expected behavior: should show a readable author name like "Test User <test@example.com>"
    // Actual behavior: shows "h_e858f2c2faea28" (the hash ID)
    // This assertion will fail until we fix it
    assert!(
        !displayed_name.starts_with("h_"),
        "Visual output should show human author name, not human ID '{}'. Full output:\n{}",
        displayed_name,
        output
    );

    // Verify line4 has the same attribution
    assert_eq!(
        line4.attribution, line3.attribution,
        "line4 should have same attribution as line3"
    );
}

#[test]
fn test_diff_json_output_includes_human_id_in_hunks() {
    let repo = TestRepo::new();

    // Create a base commit
    write_lines(&repo, "human_json.txt", &["base1", "base2"]);
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base commit");

    // Add lines with known human checkpoint
    write_lines(
        &repo,
        "human_json.txt",
        &["base1", "base2", "human1", "human2"],
    );
    checkpoint_known_human(&repo, "human_json.txt");
    let commit = commit_after_staging_all(&repo, "human additions");

    // Get JSON diff output
    let diff = diff_json(&repo, &["diff", &commit.commit_sha, "--json"]);

    // Verify the hunks array contains human_id field
    let hunks = diff["hunks"].as_array().expect("hunks should be an array");

    // Find hunks for our file
    let human_hunks: Vec<&Value> = hunks
        .iter()
        .filter(|h| h["file_path"] == "human_json.txt" && h["hunk_kind"] == "addition")
        .collect();

    assert!(!human_hunks.is_empty(), "should have addition hunks");

    // Bug: Currently human_id field is missing from hunks
    // Expected: hunks should have a "human_id" field set to the h_-prefixed hash
    // Actual: only "prompt_id" field exists (for AI), no equivalent for humans
    // This assertion will fail until we fix it
    let has_human_id = human_hunks.iter().any(|h| h.get("human_id").is_some());
    assert!(
        has_human_id,
        "At least one human hunk should have human_id field. Found hunks: {:?}",
        human_hunks
    );

    // Verify the top-level humans map is present and can resolve human_id values
    let humans = diff["humans"]
        .as_object()
        .expect("JSON should have top-level humans object");
    assert!(
        !humans.is_empty(),
        "humans map should not be empty when there are human-authored hunks"
    );

    // If we get here, verify the human_id starts with "h_" and prompt_id is not set
    // Also verify that each human_id can be resolved via the top-level humans map
    for hunk in &human_hunks {
        if let Some(human_id) = hunk.get("human_id").and_then(|v| v.as_str()) {
            assert!(
                human_id.starts_with("h_"),
                "human_id should start with h_ prefix, got: {}",
                human_id
            );
            assert!(
                hunk["prompt_id"].is_null() || !hunk.as_object().unwrap().contains_key("prompt_id"),
                "Human hunks should not have prompt_id when they have human_id"
            );

            // Verify the human_id can be resolved via the humans map
            let human_record = humans.get(human_id).unwrap_or_else(|| {
                panic!(
                    "human_id '{}' from hunk should be resolvable in top-level humans map",
                    human_id
                )
            });
            let author = human_record["author"]
                .as_str()
                .expect("human record should have author field");
            assert!(
                !author.is_empty(),
                "Resolved author name should not be empty"
            );
        }
    }
}

#[test]
fn test_diff_json_humans_map_complete_across_multiple_commits() {
    let repo = TestRepo::new();

    // Create base commit
    write_lines(&repo, "multi_human.txt", &["line1"]);
    checkpoint_human(&repo);
    let _base = commit_after_staging_all(&repo, "base");

    // Commit 1: First human author
    write_lines(
        &repo,
        "multi_human.txt",
        &["line1", "human_a_1", "human_a_2"],
    );
    checkpoint_known_human(&repo, "multi_human.txt");
    let _commit1 = commit_after_staging_all(&repo, "first human");

    // Commit 2: Second human author (creates a different h_ ID)
    write_lines(
        &repo,
        "multi_human.txt",
        &["line1", "human_a_1", "human_a_2", "human_b_1", "human_b_2"],
    );
    checkpoint_known_human(&repo, "multi_human.txt");
    let commit2 = commit_after_staging_all(&repo, "second human");

    // Get JSON diff for commit2 (which includes lines from both human checkpoints)
    let diff = diff_json(&repo, &["diff", &commit2.commit_sha, "--json"]);

    // Extract all human_ids from all hunks
    let hunks = diff["hunks"].as_array().expect("hunks should be array");
    let mut human_ids_in_hunks: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for hunk in hunks {
        if let Some(human_id) = hunk.get("human_id").and_then(|v| v.as_str()) {
            human_ids_in_hunks.insert(human_id.to_string());
        }
    }

    // Get the top-level humans map
    let humans = diff["humans"].as_object().expect("should have humans map");

    // Critical assertion: every human_id in hunks MUST be resolvable via the humans map
    for human_id in &human_ids_in_hunks {
        assert!(
            humans.contains_key(human_id),
            "human_id '{}' appears in hunks but is missing from top-level humans map. \
             Hunks reference {} unique human_ids but humans map only contains {} entries: {:?}",
            human_id,
            human_ids_in_hunks.len(),
            humans.len(),
            humans.keys().collect::<Vec<_>>()
        );

        // Also verify the author field is present and non-empty
        let author = humans[human_id]["author"]
            .as_str()
            .expect("author should be string");
        assert!(!author.is_empty(), "author name should not be empty");
    }

    // We should have collected at least one human across the commits
    assert!(
        !human_ids_in_hunks.is_empty(),
        "Should have at least one human_id across multiple commits"
    );

    // Verify humans map contains exactly the humans referenced by hunks (no orphans)
    assert_eq!(
        humans.len(),
        human_ids_in_hunks.len(),
        "humans map should contain exactly the humans referenced in hunks, no more, no less"
    );
}

crate::reuse_tests_in_worktree!(
    test_diff_visual_output_shows_human_author_name_not_id,
    test_diff_json_output_includes_human_id_in_hunks,
    test_diff_json_humans_map_complete_across_multiple_commits,
);
