use super::{ExpectedLineExt, GitAiBlameOptions, GitAiRepository, TestRepo};

#[test]
fn test_blame_success_with_line_range() {
    // Happy path: Blame with -L flag to specify line range
    let repo = TestRepo::new();
    let mut file = repo.filename("ranges.txt");

    file.set_contents(crate::lines![
        "Line 1", "Line 2", "Line 3", "Line 4", "Line 5"
    ]);

    repo.stage_all_and_commit("Multi-line file").unwrap();

    let output = repo.git_ai(&["blame", "-L", "2,4", "ranges.txt"]).unwrap();

    assert!(output.contains("Line 2"));
    assert!(output.contains("Line 3"));
    assert!(output.contains("Line 4"));
    assert!(!output.contains("Line 1"));
    assert!(!output.contains("Line 5"));
}

#[test]
fn test_blame_format_json_line_ranges() {
    // Output format: JSON format with line ranges
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1".ai(),
        "Line 2".ai(),
        "Line 3".ai(),
        "Line 4".human(),
        "Line 5".ai()
    ]);
    repo.stage_all_and_commit("Test").unwrap();

    let output = repo.git_ai(&["blame", "--json", "test.txt"]).unwrap();

    let json: serde_json::Value = serde_json::from_str(&output).expect("Should be valid JSON");

    let lines = json["lines"].as_object().unwrap();

    // Consecutive AI lines should be grouped into ranges
    // Format should be either "1" or "1-3" for ranges
    let has_range = lines.keys().any(|k| k.contains("-"));
    assert!(
        has_range || lines.len() == 1,
        "Should group consecutive lines"
    );
}

// =============================================================================
// Commit Range Tests - newest_commit, oldest_commit, oldest_date
// =============================================================================

#[test]
fn test_blame_commit_range_oldest_and_newest() {
    // Commit range: Both oldest_commit and newest_commit specified
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Version 1"]);
    let commit1 = repo.stage_all_and_commit("First").unwrap().commit_sha;

    file.set_contents(crate::lines!["Version 2"]);
    let commit2 = repo.stage_all_and_commit("Second").unwrap().commit_sha;

    file.set_contents(crate::lines!["Version 3"]);
    repo.stage_all_and_commit("Third").unwrap();

    // Blame in range commit1..commit2
    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");

    let options = GitAiBlameOptions {
        oldest_commit: Some(commit1),
        newest_commit: Some(commit2),
        ..Default::default()
    };

    let (line_authors, _) = gitai_repo.blame("test.txt", &options).unwrap();

    // Should show authorship from within the range
    assert!(!line_authors.is_empty());
}

#[test]
fn test_blame_commit_range_with_oldest_date() {
    // Commit range: Using oldest_date to limit history
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Old content"]);
    repo.stage_all_and_commit_with_env(
        "Old",
        &[
            ("GIT_AUTHOR_DATE", "2030-01-03T00:00:00Z"),
            ("GIT_COMMITTER_DATE", "2030-01-03T00:00:00Z"),
        ],
    )
    .unwrap();
    let now = chrono::DateTime::parse_from_rfc3339("2030-01-03T00:00:01Z")
        .expect("valid RFC3339 cutoff date")
        .with_timezone(&chrono::Utc);

    file.set_contents(crate::lines!["New content"]);
    repo.stage_all_and_commit_with_env(
        "New",
        &[
            ("GIT_AUTHOR_DATE", "2030-01-03T00:00:02Z"),
            ("GIT_COMMITTER_DATE", "2030-01-03T00:00:02Z"),
        ],
    )
    .unwrap();

    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");

    let options = GitAiBlameOptions {
        oldest_date: Some(now.into()),
        no_output: true,
        ..Default::default()
    };

    // Blame should only see commits after the date
    let result = gitai_repo.blame("test.txt", &options);
    assert!(result.is_ok());
}

// =============================================================================
// Contents Flag Tests - Blaming modified buffer contents
// =============================================================================

#[test]
fn test_blame_contents_modified_buffer() {
    // Contents flag: Blame modified buffer contents (uncommitted changes)
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["Original line".ai()]);
    repo.stage_all_and_commit("Original").unwrap();

    // Modified content not yet committed
    let modified = "Modified line\n";

    let output = repo
        .git_ai_with_stdin(
            &["blame", "--contents", "-", "test.txt"],
            modified.as_bytes(),
        )
        .unwrap();

    assert!(output.contains("Modified line"));
    assert!(output.contains("External file"));
}

// =============================================================================
// Multiple Line Ranges Tests
// =============================================================================

#[test]
fn test_blame_multiple_line_ranges() {
    // Multiple line ranges: Blame with multiple -L flags
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1", "Line 2", "Line 3", "Line 4", "Line 5"
    ]);
    repo.stage_all_and_commit("Five lines").unwrap();

    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");

    let options = GitAiBlameOptions {
        line_ranges: vec![(1, 2), (4, 5)],
        no_output: true,
        ..Default::default()
    };

    let (line_authors, _) = gitai_repo.blame("test.txt", &options).unwrap();

    // Should have lines 1, 2, 4, 5 but not 3
    assert!(line_authors.contains_key(&1));
    assert!(line_authors.contains_key(&2));
    assert!(line_authors.contains_key(&4));
    assert!(line_authors.contains_key(&5));
    assert!(!line_authors.contains_key(&3));
}

#[test]
fn test_blame_analysis_matches_blame_no_output_multi_ranges() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1",
        "Line 2".ai(),
        "Line 3",
        "Line 4",
        "Line 5".ai()
    ]);
    repo.stage_all_and_commit("Five lines").unwrap();

    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");

    let options = GitAiBlameOptions {
        line_ranges: vec![(1, 2), (4, 5)],
        no_output: true,
        ..Default::default()
    };

    let (line_authors, prompt_records) = gitai_repo.blame("test.txt", &options).unwrap();
    let analysis = gitai_repo.blame_analysis("test.txt", &options).unwrap();

    assert_eq!(line_authors, analysis.line_authors);
    assert_eq!(prompt_records, analysis.prompt_records);
}

#[test]
fn test_blame_analysis_returns_requested_ranges_only() {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1", "Line 2", "Line 3", "Line 4", "Line 5", "Line 6"
    ]);
    repo.stage_all_and_commit("Six lines").unwrap();

    let gitai_repo = GitAiRepository::find_repository_in_path(repo.path().to_str().unwrap())
        .expect("Failed to find repository");

    let options = GitAiBlameOptions {
        line_ranges: vec![(1, 2), (5, 6)],
        ..Default::default()
    };

    let analysis = gitai_repo.blame_analysis("test.txt", &options).unwrap();

    let mut actual_lines = std::collections::BTreeSet::new();
    for hunk in &analysis.blame_hunks {
        for line in hunk.range.0..=hunk.range.1 {
            actual_lines.insert(line);
        }
    }

    let expected_lines = std::collections::BTreeSet::from([1u32, 2, 5, 6]);
    assert_eq!(expected_lines, actual_lines);
    assert!(!actual_lines.contains(&3));
    assert!(!actual_lines.contains(&4));
}

crate::reuse_tests_in_worktree!(
    test_blame_success_with_line_range,
    test_blame_format_json_line_ranges,
    test_blame_commit_range_oldest_and_newest,
    test_blame_commit_range_with_oldest_date,
    test_blame_contents_modified_buffer,
    test_blame_multiple_line_ranges,
);
