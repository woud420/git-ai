use super::{
    DiffLine, ExpectedLineExt, TestRepo, assert_diff_line, assert_diff_lines_exact, fs,
    parse_diff_output,
};

#[test]
fn test_diff_shows_ai_attribution() {
    let repo = TestRepo::new();

    // Initial commit
    let mut file = repo.filename("ai_test.rs");
    file.set_contents(crate::lines!["fn old() {}".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    // AI makes changes
    file.set_contents(crate::lines!["fn new() {}".ai(), "fn another() {}".ai()]);
    let commit = repo.stage_all_and_commit("AI changes").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff should succeed");

    // Parse and verify exact sequence
    let lines = parse_diff_output(&output);

    // Verify exact order: deletion, then two additions
    assert_diff_lines_exact(
        &lines,
        &[
            ("-", "fn old()", None),       // Old line deleted (may have no-data or human)
            ("+", "fn new()", Some("ai")), // AI adds fn new()
            ("+", "fn another()", Some("ai")), // AI adds fn another()
        ],
    );
}

#[test]
fn test_diff_shows_human_attribution() {
    let repo = TestRepo::new();

    // Initial commit
    let mut file = repo.filename("human_test.rs");
    file.set_contents(crate::lines!["fn old() {}".ai()]);
    repo.stage_all_and_commit("Initial AI").unwrap();

    // Human makes changes
    file.set_contents(crate::lines![
        "fn new() {}".human(),
        "fn another() {}".human()
    ]);
    let commit = repo.stage_all_and_commit("Human changes").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff should succeed");

    // Parse and verify exact sequence
    let lines = parse_diff_output(&output);

    // Verify exact order: deletion, then two additions
    assert_eq!(lines.len(), 3, "Should have exactly 3 lines");

    // First line: deletion (no attribution on deletions)
    assert_diff_line(&lines[0], "-", "fn old()", None);

    // Next two lines: additions (will have no-data or human attribution)
    assert_diff_line(&lines[1], "+", "fn new()", None);
    assert_diff_line(&lines[2], "+", "fn another()", None);

    // Verify both additions have some attribution
    assert!(
        lines[1].attribution.is_some(),
        "First addition should have attribution"
    );
    assert!(
        lines[2].attribution.is_some(),
        "Second addition should have attribution"
    );
}

#[test]
fn test_diff_pure_additions() {
    let repo = TestRepo::new();

    // Initial commit with one line
    let mut file = repo.filename("additions.txt");
    file.set_contents(crate::lines!["Line 1".human()]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Add more lines at the end (pure additions)
    file.set_contents(crate::lines![
        "Line 1".human(),
        "Line 2".ai(),
        "Line 3".ai()
    ]);
    let commit = repo.stage_all_and_commit("Add lines").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff should succeed");

    // Should have additions
    assert!(
        output.contains("+Line 2") || output.contains("Line 2"),
        "Should show Line 2 addition"
    );
    assert!(
        output.contains("+Line 3") || output.contains("Line 3"),
        "Should show Line 3 addition"
    );

    // Should show AI attribution on added lines
    assert!(
        output.contains("🤖") || output.contains("mock_ai"),
        "Should show AI attribution on additions"
    );
}

#[test]
fn test_diff_pure_deletions() {
    let repo = TestRepo::new();

    // Initial commit with multiple lines
    let mut file = repo.filename("deletions.txt");
    file.set_contents(crate::lines![
        "Line 1".ai(),
        "Line 2".ai(),
        "Line 3".human(),
        "Line 4".ai()
    ]);
    repo.stage_all_and_commit("Initial").unwrap();

    // Delete all lines
    file.set_contents(crate::lines![]);
    let commit = repo.stage_all_and_commit("Delete all").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff should succeed");

    // Parse and verify exact sequence
    let lines = parse_diff_output(&output);

    // Verify exact order: 4 deletions in sequence, no additions
    assert_eq!(
        lines.len(),
        4,
        "Should have exactly 4 lines (all deletions)"
    );

    assert_diff_lines_exact(
        &lines,
        &[
            ("-", "Line 1", None), // No attribution on deletions
            ("-", "Line 2", None), // No attribution on deletions
            ("-", "Line 3", None), // No attribution on deletions
            ("-", "Line 4", None), // No attribution on deletions
        ],
    );
}

#[test]
fn test_diff_mixed_ai_and_human() {
    let repo = TestRepo::new();

    // Initial commit with AI content
    let mut file = repo.filename("mixed.txt");
    file.set_contents(crate::lines!["Line 1".ai(), "Line 2".ai()]);
    repo.stage_all_and_commit("Initial AI").unwrap();

    // Modify with AI changes
    file.set_contents(crate::lines![
        "Line 1".ai(),
        "Line 2 modified".ai(),
        "Line 3 new".ai()
    ]);
    let commit = repo.stage_all_and_commit("AI modifies").unwrap();

    // Run diff
    let output = repo
        .git_ai(&["diff", &commit.commit_sha])
        .expect("git-ai diff should succeed");

    // Should have both additions and deletions
    assert!(output.contains("-"), "Should have deletion lines");
    assert!(output.contains("+"), "Should have addition lines");

    // Should show AI attribution
    let has_ai = output.contains("🤖") || output.contains("mock_ai");
    assert!(has_ai, "Should show AI attribution, output:\n{}", output);
}

/// Regression test: when AI reorders functions in a file, moved lines must be
/// AI-attributed.  Uses direct file writes + checkpoint calls instead of the
/// two-pass `set_contents` helper.
///
/// Scenario
/// --------
/// Commit A (AI):   [func_one, func_two] — fully AI-attested via checkpoint.
/// Commit B (AI):   [new_func, func_two, func_one] — AI adds new_func and
///                  moves func_one to the end.  A single checkpoint covers
///                  the whole change.
///
/// Myers diff A→B shows func_one at its new position as `+` lines.
/// Because B's checkpoint attributed the full before→after diff to AI,
/// the authorship note covers those lines and git ai diff shows them as AI.
#[test]
fn test_diff_moved_ai_lines_attributed_correctly() {
    let repo = TestRepo::new();

    // --- Commit A: AI writes two functions (fully AI-attested) ---
    let file_path = repo.path().join("src.rs");
    let initial_content = "\
fn func_one() {
    // original function one body
    let x: u32 = 1;
    let y: u32 = 2;
    x + y
}
fn func_two() {
    // original function two body
    let a = String::from(\"hello\");
    let b = String::from(\"world\");
    format!(\"{} {}\", a, b)
}";
    fs::write(&file_path, initial_content).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
    repo.stage_all_and_commit("A: AI writes func_one and func_two")
        .unwrap();

    // --- Commit B: AI adds new_func at top and moves func_one to end ---
    let reordered_content = "\
fn new_func() {
    // brand new function
    let z: u32 = 99;
    let w: u32 = 100;
    z + w
}
fn func_two() {
    // original function two body
    let a = String::from(\"hello\");
    let b = String::from(\"world\");
    format!(\"{} {}\", a, b)
}
fn func_one() {
    // original function one body
    let x: u32 = 1;
    let y: u32 = 2;
    x + y
}";
    fs::write(&file_path, reordered_content).unwrap();
    // Single AI checkpoint: diffs initial_content → reordered_content.
    // func_one at the bottom is an Insert → attributed to AI.
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
    let commit_b = repo
        .stage_all_and_commit("B: AI adds new_func and moves func_one to end")
        .unwrap();

    // Every line in the file should be AI-attributed via blame.
    let mut file = repo.filename("src.rs");
    file.assert_lines_and_blame(crate::lines![
        "fn new_func() {".ai(),
        "    // brand new function".ai(),
        "    let z: u32 = 99;".ai(),
        "    let w: u32 = 100;".ai(),
        "    z + w".ai(),
        "}".ai(),
        "fn func_two() {".ai(),
        "    // original function two body".ai(),
        "    let a = String::from(\"hello\");".ai(),
        "    let b = String::from(\"world\");".ai(),
        "    format!(\"{} {}\", a, b)".ai(),
        "}".ai(),
        "fn func_one() {".ai(),
        "    // original function one body".ai(),
        "    let x: u32 = 1;".ai(),
        "    let y: u32 = 2;".ai(),
        "    x + y".ai(),
        "}".ai()
    ]);

    // Confirm the Myers diff actually puts func_one as explicit `+` lines.
    let raw_diff = repo
        .git_og(&[
            "--no-pager",
            "diff",
            &format!("{}^", commit_b.commit_sha),
            &commit_b.commit_sha,
        ])
        .expect("git diff should succeed");
    assert!(
        raw_diff.contains("+fn func_one() {"),
        "precondition: Myers diff must show func_one as an explicit addition (+), got:\n{raw_diff}"
    );

    // Run git-ai diff and check attributions.
    let output = repo
        .git_ai(&["diff", &commit_b.commit_sha])
        .expect("git-ai diff should succeed");

    let lines = parse_diff_output(&output);

    // new_func must be AI (directly in B's attestation).
    let new_func_line = lines
        .iter()
        .find(|l| l.prefix == "+" && l.content.contains("fn new_func()"))
        .expect("diff output must contain +fn new_func()");
    assert!(
        new_func_line
            .attribution
            .as_ref()
            .map(|a| a.contains("ai"))
            .unwrap_or(false),
        "new_func should be AI-attributed; got: {:?}",
        new_func_line.attribution
    );

    // func_one at its moved position must also be AI (checkpoint covered
    // the full before→after diff, so the insertion is AI-attributed).
    let func_one_line = lines
        .iter()
        .find(|l| l.prefix == "+" && l.content.contains("fn func_one()"))
        .expect("diff output must contain +fn func_one() from its moved position");
    assert!(
        func_one_line
            .attribution
            .as_ref()
            .map(|a| a.contains("ai"))
            .unwrap_or(false),
        "func_one (moved to end of file by commit B) should be AI-attributed, \
         but got: {:?}\nFull diff output:\n{}",
        func_one_line.attribution,
        output
    );

    // No line should show [no-data].
    let no_data_lines: Vec<&DiffLine> = lines
        .iter()
        .filter(|l| {
            l.attribution
                .as_ref()
                .map(|a| a.contains("no-data"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        no_data_lines.is_empty(),
        "No lines should have [no-data] attribution, but found {} lines: {:?}",
        no_data_lines.len(),
        no_data_lines
    );
}

crate::reuse_tests_in_worktree!(
    test_diff_shows_ai_attribution,
    test_diff_shows_human_attribution,
    test_diff_pure_additions,
    test_diff_pure_deletions,
    test_diff_mixed_ai_and_human,
);
