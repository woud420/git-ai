use super::{AuthorType, ExpectedLineExt, TestRepo, fs};

/// Test that resetting a large commit (500+ lines across many files) preserves AI
/// authorship correctly.  This is the scenario from issue #1025: the previous
/// implementation ran `git blame target..target` for every changed file in the
/// post-reset hook, which (a) is O(files × file_size) wasted work and (b) always
/// produced zero AI attributions because the range is empty.  The fix creates an
/// empty target VA directly, halving the blame work with no correctness change.
#[test]
fn test_reset_large_commit_preserves_attribution() {
    let repo = TestRepo::new();

    // Create a base commit
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base line"]);
    let base_commit = repo.stage_all_and_commit("Base").unwrap();

    // Create a large AI commit: 10 files, each with multiple AI lines (≥50 lines total)
    let file_count = 10;
    let ai_lines_per_file = 6;
    let mut file_handles = Vec::new();
    for i in 0..file_count {
        let name = format!("module_{i}.rs");
        let mut f = repo.filename(&name);
        // One human line per file so the file has a non-AI context
        f.set_contents(crate::lines![format!("// module {i}")]);
        // Insert several AI lines
        for j in 0..ai_lines_per_file {
            f.insert_at(
                (j + 1) as usize,
                crate::lines![format!("    // AI line {j} in module {i}").ai()],
            );
        }
        file_handles.push((name, f));
    }
    repo.stage_all_and_commit("Large AI commit (500+ lines)")
        .unwrap();

    // Reset the large AI commit
    repo.git(&["reset", "--mixed", &base_commit.commit_sha])
        .expect("reset --mixed should succeed");

    // Re-commit and verify all AI attributions were preserved across all files
    let new_commit = repo.stage_all_and_commit("Re-commit after reset").unwrap();

    assert!(
        !new_commit.authorship_log.attestations.is_empty(),
        "AI authorship should be preserved after resetting a large commit"
    );

    // Spot-check: each module file should still have AI-attributed lines
    for (name, _) in &file_handles {
        let f = repo.filename(name);
        let ai_lines = f.lines_by_author(AuthorType::Ai);
        assert!(
            !ai_lines.is_empty(),
            "file {name} should have AI-attributed lines after reset + re-commit"
        );
    }
}

/// Test soft-reset-recommit preserves secondary file attribution when only
/// primary file is edited between reset and recommit. Reproduces the pattern
/// from fuzz_chaos_99 where multi-file commit → soft reset → edit one file → recommit
/// loses attribution for the untouched secondary file.
#[test]
fn test_soft_reset_recommit_preserves_secondary_file() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let sec_path = repo.path().join("secondary.txt");

    // Initial commit with main file
    fs::write(&main_path, "AAA\nAAA\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Create secondary file with AI content and commit both
    fs::write(&sec_path, "BBB\nBBB\nBBB\nBBB\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();
    // Also edit main
    fs::write(&main_path, "AAA\nAAA\nCCC\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("multi-file commit").unwrap();

    // Verify secondary before soft-reset
    let mut sec_file = repo.filename("secondary.txt");
    sec_file.assert_committed_lines(crate::lines![
        "BBB".ai(),
        "BBB".ai(),
        "BBB".ai(),
        "BBB".ai(),
    ]);

    // Soft reset
    repo.git(&["reset", "--soft", "HEAD~1"]).unwrap();

    // Edit only main file
    fs::write(&main_path, "AAA\nAAA\nCCC\nDDD\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Recommit
    repo.stage_all_and_commit("recommit after soft reset")
        .unwrap();

    // Secondary file should still have full AI attribution
    sec_file.assert_committed_lines(crate::lines![
        "BBB".ai(),
        "BBB".ai(),
        "BBB".ai(),
        "BBB".ai(),
    ]);
}

/// Test that attribution survives: multi-file commit → soft-reset-recommit →
/// further edits to one file with prepends → commit. The prepends shift line
/// numbers but the untouched lines should still be properly attributed by
/// tracing back through blame.
#[test]
fn test_soft_reset_recommit_with_subsequent_prepend() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let sec_path = repo.path().join("secondary.txt");

    // Initial commit
    fs::write(&main_path, "base\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Commit secondary file with AI lines
    fs::write(&sec_path, "AI1\nAI2\nAI3\nAI4\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();
    fs::write(&main_path, "base\nedit1\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("add secondary").unwrap();

    // Soft reset and recommit (with extra edit to main only)
    repo.git(&["reset", "--soft", "HEAD~1"]).unwrap();
    fs::write(&main_path, "base\nedit1\nextra\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("recommit").unwrap();

    // Verify secondary still attributed
    let mut sec_file = repo.filename("secondary.txt");
    sec_file.assert_committed_lines(crate::lines![
        "AI1".ai(),
        "AI2".ai(),
        "AI3".ai(),
        "AI4".ai(),
    ]);

    // Now prepend to secondary and commit — old AI lines shift down
    fs::write(&sec_path, "NEW1\nNEW2\nNEW3\nAI1\nAI2\nAI3\nAI4\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();
    repo.stage_all_and_commit("prepend to secondary").unwrap();

    // Lines 4-7 should still be AI (traced back to recommit via blame)
    sec_file.assert_committed_lines(crate::lines![
        "NEW1".ai(),
        "NEW2".ai(),
        "NEW3".ai(),
        "AI1".ai(),
        "AI2".ai(),
        "AI3".ai(),
        "AI4".ai(),
    ]);
}

/// Reproduces the fuzz_chaos_99 pattern where identical content (same char
/// repeated) across multiple commits causes git blame to misattribute shifted
/// lines to a newer commit. The note for that commit must still cover them.
///
/// Pattern: AI file created → committed → soft-reset → recommit → further edits
/// with ReplaceRandom + Prepend → commit. Git blame assigns the shifted-but-
/// unchanged AI lines to the last commit, but since they weren't re-checkpointed,
/// they're missing from the note.
#[test]
fn test_blame_identical_content_shift_attribution() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let sec_path = repo.path().join("secondary.txt");

    // Initial commit
    fs::write(&main_path, "base\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Create secondary file: 8 lines of "X" (AI) — identical content per line
    fs::write(&sec_path, "X\nX\nX\nX\nX\nX\nX\nX\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();
    fs::write(&main_path, "base\nedit\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("add secondary with AI").unwrap();

    // Soft reset + recommit (only edit main)
    repo.git(&["reset", "--soft", "HEAD~1"]).unwrap();
    fs::write(&main_path, "base\nedit\nextra\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("recommit").unwrap();

    // Verify: all 8 "X" lines should be AI
    let mut sec_file = repo.filename("secondary.txt");
    sec_file.assert_committed_lines(crate::lines![
        "X".ai(),
        "X".ai(),
        "X".ai(),
        "X".ai(),
        "X".ai(),
        "X".ai(),
        "X".ai(),
        "X".ai(),
    ]);

    // Now: replace lines 1-2 with "Y" (AI), then prepend 4 "Z" lines (AI)
    // After: Z Z Z Z Y Y X X X X X X (12 lines)
    // The "X" lines at 5-12 were at 3-8 in parent → git blame should trace back.
    // But with identical "X" content and the replacement of lines 1-2, git's
    // diff algorithm may assign some "X" lines to this commit.
    fs::write(&sec_path, "Y\nY\nX\nX\nX\nX\nX\nX\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();
    fs::write(&sec_path, "Z\nZ\nZ\nZ\nY\nY\nX\nX\nX\nX\nX\nX\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();
    repo.stage_all_and_commit("replace and prepend").unwrap();

    // All lines should still be AI-attributed regardless of which commit
    // git blame assigns them to.
    sec_file.assert_committed_lines(crate::lines![
        "Z".ai(),
        "Z".ai(),
        "Z".ai(),
        "Z".ai(),
        "Y".ai(),
        "Y".ai(),
        "X".ai(),
        "X".ai(),
        "X".ai(),
        "X".ai(),
        "X".ai(),
        "X".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(test_reset_large_commit_preserves_attribution,);
