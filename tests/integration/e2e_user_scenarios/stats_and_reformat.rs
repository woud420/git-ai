use super::{ExpectedLineExt, TestRepo, extract_json_object, fs, head_stats};

// ---------------------------------------------------------------------------
// Test 13: Two AI commits, reset last commit, then recommit (SKIPPED — issue #169)
// ---------------------------------------------------------------------------
#[test]
#[ignore = "https://github.com/git-ai-project/git-ai/issues/169"]
fn test_reset_and_recommit_preserves_authorship() {
    let _repo = TestRepo::new();
}

// ---------------------------------------------------------------------------
// Test 16: git-ai stats range command
// ---------------------------------------------------------------------------
#[test]
fn test_stats_range_command() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("example.txt");

    // Create an anchor commit so we have a valid HEAD
    fs::write(repo.path().join("README.md"), "# Test\n").unwrap();
    repo.git(&["add", "README.md"]).unwrap();
    repo.commit("Initial commit").unwrap();

    let base_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Commit 1: Human adds 3 lines
    let human_content = "\
H: Human Line 1
H: Human Line 2
H: Human Line 3
";
    repo.human_edit("example.txt", human_content);
    repo.git(&["add", "example.txt"]).unwrap();
    repo.commit("Commit 1: Human adds 3 lines").unwrap();

    let commit1_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Commit 2: AI adds 5 more lines
    let ai_content = "\
H: Human Line 1
H: Human Line 2
H: Human Line 3
AI: AI Line 1
AI: AI Line 2
AI: AI Line 3
AI: AI Line 4
AI: AI Line 5
";
    fs::write(&file_path, ai_content).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();
    repo.git(&["add", "example.txt"]).unwrap();
    repo.commit("Commit 2: AI adds 5 lines").unwrap();

    let commit2_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Test range: base_commit..commit2 (includes both commits)
    let range = format!("{base_sha}..{commit2_sha}");
    let raw = repo
        .git_ai(&["stats", &range, "--json"])
        .expect("stats range should succeed");
    let json = extract_json_object(&raw);
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

    let range_stats = &parsed["range_stats"];
    // Range stats re-compute attribution against the range boundary, so known_human
    // attestation from individual commits may become unknown in the range view.
    let total_human = range_stats["human_additions"].as_u64().unwrap_or(0)
        + range_stats["unknown_additions"].as_u64().unwrap_or(0);
    assert_eq!(total_human, 3, "total non-AI additions in range");
    assert_eq!(range_stats["ai_additions"], 5);
    assert_eq!(range_stats["ai_accepted"], 5);
    assert_eq!(range_stats["git_diff_deleted_lines"], 0);
    assert_eq!(range_stats["git_diff_added_lines"], 8);
    assert_eq!(
        range_stats["tool_model_breakdown"]["mock_ai::unknown"]["ai_additions"],
        5
    );
    assert_eq!(
        range_stats["tool_model_breakdown"]["mock_ai::unknown"]["ai_accepted"],
        5
    );

    let authorship_stats = &parsed["authorship_stats"];
    assert_eq!(authorship_stats["total_commits"], 2);
    assert_eq!(authorship_stats["commits_with_authorship"], 2);

    // Test narrower range: commit1..commit2 (only commit 2)
    let range_single = format!("{commit1_sha}..{commit2_sha}");
    let raw_single = repo
        .git_ai(&["stats", &range_single, "--json"])
        .expect("stats range should succeed");
    let json_single = extract_json_object(&raw_single);
    let parsed_single: serde_json::Value = serde_json::from_str(&json_single).expect("valid JSON");

    let range_stats_single = &parsed_single["range_stats"];
    assert_eq!(range_stats_single["ai_additions"], 5);
    assert_eq!(range_stats_single["ai_accepted"], 5);

    assert_eq!(parsed_single["authorship_stats"]["total_commits"], 1);
}

// ---------------------------------------------------------------------------
// Test: Issue #394 — Multi-user collaboration with code reformatting and AI
//
// Scenario from https://github.com/git-ai-project/git-ai/issues/394:
// 1. User test-a creates a single-line function and commits
// 2. User test-b checkpoints, reformats + wraps that function with AI lines,
//    checkpoints as AI, and commits
// 3. Verify blame attribution after each commit
// ---------------------------------------------------------------------------
#[test]
fn test_issue_394_multiuser_reformat_ai_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("hello.js");

    // --- Commit 1: user test-a creates the file ---
    repo.git(&["config", "user.name", "test-a"]).unwrap();
    repo.git(&["config", "user.email", "test-a@example.com"])
        .unwrap();

    let initial = "function hello() {console.log('hello')}\n";
    fs::write(&file_path, initial).unwrap();
    // No checkpoints — this is a plain untracked commit
    repo.stage_all_and_commit("Initial commit by test-a")
        .unwrap();

    let mut file = repo.filename("hello.js");
    file.assert_committed_lines(lines![
        "function hello() {console.log('hello')}".unattributed_human(),
    ]);

    // --- Commit 2: user test-b reformats + adds AI lines ---
    repo.git(&["config", "user.name", "test-b"]).unwrap();
    repo.git(&["config", "user.email", "test-b@example.com"])
        .unwrap();

    // Pre-edit checkpoint (untracked/legacy human) — mimics AI agent preset's
    // before-edit snapshot to exclude prior changes
    repo.git_ai(&["checkpoint", "human", "hello.js"]).unwrap();

    let modified = "\
console.log('a')
function hello() {
    console.log('hello')
}
console.log('b')
";
    fs::write(&file_path, modified).unwrap();

    // Post-edit AI checkpoint
    repo.git_ai(&["checkpoint", "mock_ai", "hello.js"]).unwrap();

    repo.stage_all_and_commit("AI-assisted edit by test-b")
        .unwrap();

    // Verify blame: the 2 new wrapper lines should be AI, and the 3
    // reformatted function lines' attribution is what issue #394 questions.
    let blame_output = repo.git_ai(&["blame", "hello.js"]).unwrap();
    eprintln!("=== git-ai blame output (issue #394) ===\n{blame_output}");

    let stats = head_stats(&repo);
    eprintln!(
        "=== commit stats (issue #394) ===\nhuman_additions={}, ai_additions={}, ai_accepted={}",
        stats.human_additions, stats.ai_additions, stats.ai_accepted
    );

    // Issue #394 reported 60% test-b / 40% AI (3 reformatted function lines
    // attributed to the committer instead of AI).  Current behaviour: all 5
    // lines are attributed to AI (the entire diff between the pre-edit and
    // post-edit checkpoints is AI), so the original split no longer reproduces.
    let mut file = repo.filename("hello.js");
    file.assert_committed_lines(lines![
        "console.log('a')".ai(),
        "function hello() {".ai(),
        "    console.log('hello')".ai(),
        "}".ai(),
        "console.log('b')".ai(),
    ]);
}
