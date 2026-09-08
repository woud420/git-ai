use super::{ExpectedLineExt, TestRepo, files_in_note, total_accepted_lines};

// ---------------------------------------------------------------------------
// Test 5: line-level blame + accepted_lines correctness after slow-path rebase
// ---------------------------------------------------------------------------

/// After a slow-path rebase the per-line AI blame attribution must be correct
/// and `accepted_lines` for an intermediate commit must be strictly less than
/// for the tip commit.
///
/// Two-commit chain, both appending to a file whose upstream prepended a line.
///   - A′ note: must NOT contain separate.rs (introduced by B)
///   - accepted_lines for A′ < accepted_lines for B′
///   - Line-level blame on main.rs reflects the expected attribution
#[test]
fn test_rebase_slow_path_line_attribution_is_correct() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("main.rs", "fn original() {}", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream prepends to main.rs → forces slow path.
    repo.commit_untracked_file(
        "main.rs",
        "// upstream\nfn original() {}",
        "Upstream: prepend to main.rs",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    let mut main_rs = repo.filename("main.rs");

    // Commit A: appends 3 AI lines to main.rs (no separate.rs yet).
    main_rs.set_contents(crate::lines![
        "fn original() {}",
        "fn ai_a1() {}".ai(),
        "fn ai_a2() {}".ai(),
        "fn ai_a3() {}".ai()
    ]);
    repo.stage_all_and_commit("Commit A: 3 AI lines").unwrap();

    // Commit B: appends 3 more AI lines to main.rs + adds separate.rs.
    main_rs.set_contents(crate::lines![
        "fn original() {}",
        "fn ai_a1() {}".ai(),
        "fn ai_a2() {}".ai(),
        "fn ai_a3() {}".ai(),
        "fn ai_b1() {}".ai(),
        "fn ai_b2() {}".ai(),
        "fn ai_b3() {}".ai()
    ]);
    let mut separate = repo.filename("separate.rs");
    separate.set_contents(crate::lines!["fn sep() {}".ai()]);
    repo.stage_all_and_commit("Commit B: 3 more AI lines + separate.rs")
        .unwrap();

    repo.git(&["rebase", &default_branch])
        .expect("rebase should succeed without conflicts");

    let sha_b = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let sha_a = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();

    let note_a = repo
        .read_authorship_note(&sha_a)
        .expect("commit A′ should have a note");
    let note_b = repo
        .read_authorship_note(&sha_b)
        .expect("commit B′ should have a note");

    let files_a = files_in_note(&note_a);

    // A′: separate.rs was introduced in B (future) — must not appear in A.
    assert!(
        !files_a.iter().any(|f| f.contains("separate")),
        "REBASE NOTE CORRUPTION: commit A′'s note contains 'separate.rs', \
         which was only introduced in commit B. Files in A′: {:?}",
        files_a
    );

    // accepted_lines for A′ (3 lines) must be less than B′ (7 lines).
    let lines_a = total_accepted_lines(&note_a);
    let lines_b = total_accepted_lines(&note_b);
    assert!(
        lines_a < lines_b,
        "REBASE NOTE CORRUPTION: commit A′ has accepted_lines={} and \
         commit B′ has accepted_lines={}. A′ came before B′ and introduced \
         fewer AI lines, so A′ should have a strictly smaller count.",
        lines_a,
        lines_b
    );

    // Verify line-level blame reflects the upstream header + AI appended lines.
    main_rs.assert_lines_and_blame(crate::lines![
        "// upstream",
        "fn original() {}",
        "fn ai_a1() {}".ai(),
        "fn ai_a2() {}".ai(),
        "fn ai_a3() {}".ai(),
        "fn ai_b1() {}".ai(),
        "fn ai_b2() {}".ai(),
        "fn ai_b3() {}".ai()
    ]);
}

// ---------------------------------------------------------------------------
// Test 6: AI lines newly added in commit K≥2 must not be attributed as human
// ---------------------------------------------------------------------------

/// The hunk-based path (used for all new commits after the first content-diff)
/// only carries EXISTING attributions forward by shifting line numbers. It does
/// NOT stamp newly-inserted lines with attribution. This means any AI line that
/// is "new" in commit B relative to commit A (i.e., was inserted in the A′→B′ diff)
/// will appear as human in B′'s blame, even though the original commit B had it
/// 100% AI.
///
/// Setup:
///   - Upstream prepends `// upstream` to shared.rs (forces slow path for all commits)
///   - Commit A: appends `fn ai_a()` to shared.rs (AI)
///   - Commit B: appends `fn ai_b()` to shared.rs (AI) — this line is "inserted" in A′→B′
///
/// After rebase:
///   - A′ is processed via content-diff (first commit, correct)
///   - B′ is processed via hunk-based path
///     → hunk-based path shifts fn_ai_a's attribution (line offset +0) ✓
///     → hunk-based path sees fn_ai_b as an "inserted" line → assigns NO attribution ✗
///   - `git ai diff B′` (blame) shows fn_ai_b as human even though it's 100% AI
#[test]
fn test_rebase_hunk_path_does_not_drop_ai_attribution_for_new_lines() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("shared.rs", "fn original() {}", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream prepends — forces slow path for all feature commits.
    repo.commit_untracked_file(
        "shared.rs",
        "// upstream\nfn original() {}",
        "Upstream: prepend to shared.rs",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    let mut shared = repo.filename("shared.rs");

    // Commit A: appends fn_ai_a (AI).
    shared.set_contents(crate::lines!["fn original() {}", "fn ai_a() {}".ai()]);
    repo.stage_all_and_commit("Commit A: fn ai_a").unwrap();

    // Commit B: appends fn_ai_b (AI).
    // After rebase, B′'s diff vs A′ shows fn_ai_b as an "inserted" line.
    // The hunk-based path has no way to stamp inserted lines as AI → bug.
    shared.set_contents(crate::lines![
        "fn original() {}",
        "fn ai_a() {}".ai(),
        "fn ai_b() {}".ai()
    ]);
    repo.stage_all_and_commit("Commit B: fn ai_b").unwrap();

    repo.git(&["rebase", &default_branch])
        .expect("rebase should succeed without conflicts");

    // After rebase, shared.rs at HEAD (B′) should have:
    //   line 1: // upstream  ← human (added by upstream)
    //   line 2: fn original() {}  ← human
    //   line 3: fn ai_a() {}  ← AI (from commit A, correctly preserved)
    //   line 4: fn ai_b() {}  ← AI (from commit B, DROPPED by hunk-based path)
    //
    // The assert_lines_and_blame call tests the per-line blame attribution.
    // If fn ai_b is attributed as human, this will fail with the right message.
    shared.assert_lines_and_blame(crate::lines![
        "// upstream",
        "fn original() {}",
        "fn ai_a() {}".ai(),
        "fn ai_b() {}".ai() // BUG: hunk-based path drops this → shown as human
    ]);
}

// ---------------------------------------------------------------------------
// Test 7: attribution loss for the second-commit's per-note accepted_lines
// ---------------------------------------------------------------------------

/// Stronger variant: verify via note inspection that B′'s note attributes
/// fn_ai_b as AI. The blame test above checks line-level; this checks the
/// stored note directly. Both fail with the current buggy code.
#[test]
fn test_rebase_second_commit_note_attributes_its_own_ai_lines() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("work.rs", "fn base() {}", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream prepends → slow path for all commits.
    repo.commit_untracked_file(
        "work.rs",
        "// header\nfn base() {}",
        "Upstream: prepend to work.rs",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    let mut work = repo.filename("work.rs");

    // Commit A: 3 AI lines.
    work.set_contents(crate::lines![
        "fn base() {}",
        "fn a1() {}".ai(),
        "fn a2() {}".ai(),
        "fn a3() {}".ai()
    ]);
    repo.stage_all_and_commit("Commit A: 3 AI lines").unwrap();

    // Commit B: 3 more AI lines (different functions so there's no overlap with A).
    work.set_contents(crate::lines![
        "fn base() {}",
        "fn a1() {}".ai(),
        "fn a2() {}".ai(),
        "fn a3() {}".ai(),
        "fn b1() {}".ai(),
        "fn b2() {}".ai(),
        "fn b3() {}".ai()
    ]);
    repo.stage_all_and_commit("Commit B: 3 more AI lines")
        .unwrap();

    repo.git(&["rebase", &default_branch])
        .expect("rebase should succeed without conflicts");

    let sha_b = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let sha_a = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();

    let note_a = repo
        .read_authorship_note(&sha_a)
        .expect("commit A′ should have a note");
    let note_b = repo
        .read_authorship_note(&sha_b)
        .expect("commit B′ should have a note");

    let lines_a = total_accepted_lines(&note_a);
    let lines_b = total_accepted_lines(&note_b);
    // Each commit's note attributes the AI lines that IT introduced (per diff from parent).
    // A′: introduced a1-a3 over base → 3 AI lines (plus base line in committed_hunks due to
    //     trailing-newline, but base has no AI attribution) → 3.
    // B′: introduced b1-b3 over A, plus a3 appears in committed_hunks due to trailing-newline
    //     diff handling, and a3 IS in the AI checkpoint → 4 AI lines.
    assert_eq!(
        lines_a, 3,
        "A′ should have exactly 3 AI lines (fn a1..a3), got {}.",
        lines_a
    );
    assert_eq!(
        lines_b, 4,
        "B′ should have 4 AI lines (fn a3 + fn b1..b3 in committed_hunks), got {}.",
        lines_b
    );
}

// ---------------------------------------------------------------------------
// Test 8: three-commit chain — attribution loss compounds across commits
// ---------------------------------------------------------------------------

/// Three-commit feature chain (A, B, C) each appending to the same file.
/// Upstream prepends (forces slow path). Only commit A′ is processed via
/// content-diff; B′ and C′ use the hunk-based path.
///
/// Expected: each commit's note includes ONLY its own newly-added AI lines.
/// Broken: B′ and C′ notes don't include their own new AI lines at all
/// (they only retain A's lines shifted by offset).
#[test]
fn test_rebase_attribution_loss_compounds_across_three_commits() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("lib.rs", "fn base() {}", "Initial commit");
    let default_branch = repo.current_branch();

    repo.commit_untracked_file(
        "lib.rs",
        "// upstream\nfn base() {}",
        "Upstream: prepend to lib.rs",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    let mut lib = repo.filename("lib.rs");

    // Commit A: adds fn_a (2 AI lines).
    lib.set_contents(crate::lines![
        "fn base() {}",
        "fn a1() {}".ai(),
        "fn a2() {}".ai()
    ]);
    repo.stage_all_and_commit("Commit A: 2 AI lines").unwrap();

    // Commit B: adds fn_b (2 AI lines on top of A).
    lib.set_contents(crate::lines![
        "fn base() {}",
        "fn a1() {}".ai(),
        "fn a2() {}".ai(),
        "fn b1() {}".ai(),
        "fn b2() {}".ai()
    ]);
    repo.stage_all_and_commit("Commit B: 2 more AI lines")
        .unwrap();

    // Commit C: adds fn_c (2 AI lines on top of B).
    lib.set_contents(crate::lines![
        "fn base() {}",
        "fn a1() {}".ai(),
        "fn a2() {}".ai(),
        "fn b1() {}".ai(),
        "fn b2() {}".ai(),
        "fn c1() {}".ai(),
        "fn c2() {}".ai()
    ]);
    repo.stage_all_and_commit("Commit C: 2 more AI lines")
        .unwrap();

    repo.git(&["rebase", &default_branch])
        .expect("rebase should succeed without conflicts");

    // Line-level blame at HEAD (C′) — all 6 AI lines should be attributed as AI.
    lib.assert_lines_and_blame(crate::lines![
        "// upstream",
        "fn base() {}",
        "fn a1() {}".ai(),
        "fn a2() {}".ai(),
        "fn b1() {}".ai(), // BUG: hunk path drops this in B′ processing
        "fn b2() {}".ai(), // BUG: hunk path drops this in B′ processing
        "fn c1() {}".ai(), // BUG: hunk path drops this in C′ processing
        "fn c2() {}".ai()  // BUG: hunk path drops this in C′ processing
    ]);
}

// ---------------------------------------------------------------------------
// Test 9: same-file consecutive commits where B overwrites A's AI line
// ---------------------------------------------------------------------------

/// Two commits both touch the same file; commit B overwrites a line that commit A introduced.
///
/// Commit A: APPENDS `fn compute() { return 42; }` (AI) to the file.
/// Commit B: CHANGES that same line to `fn compute() { return 100; }` (AI).
///
/// After rebase (upstream prepends a header, forcing the slow path for A′):
/// - A′: `fn compute() { return 42; }` is AI-attributed via `original_head_line_to_author`
///   lookup in the content-diff/slow path.  BUT since the feature tip has `return 100`,
///   `return 42` is NOT in the original-HEAD content map.  A′ may or may not have a note.
/// - B′: `fn compute() { return 100; }` IS in the original-HEAD content map (it's the
///   feature tip).  The hunk-based Replace lookup correctly attributes it as AI.
///
/// Regression: if the hunk-based content-map lookup for Replace/Insert hunks were broken,
/// B′ would show `return 100` as human because `apply_hunks_to_line_attributions` alone
/// only shifts existing attributions and does not stamp newly inserted/replaced lines.
#[test]
fn test_rebase_same_line_overwritten_by_consecutive_commits() {
    let repo = TestRepo::new();

    // Initial: a file with one human line.
    repo.commit_untracked_file("compute.rs", "fn base() {}", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream prepends a module comment → forces slow path for A′.
    repo.commit_untracked_file(
        "compute.rs",
        "// module\nfn base() {}",
        "Upstream: prepend comment",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    let mut compute = repo.filename("compute.rs");

    // Commit A: APPENDS a new AI function with return value 42.
    compute.set_contents(crate::lines![
        "fn base() {}",
        "fn compute() -> u32 { return 42; }".ai(),
    ]);
    repo.stage_all_and_commit("A: add compute() returning 42")
        .unwrap();

    // Commit B: CHANGES the return value from 42 to 100 (overwrites A's line with AI content).
    compute.set_contents(crate::lines![
        "fn base() {}",
        "fn compute() -> u32 { return 100; }".ai(),
    ]);
    repo.stage_all_and_commit("B: change compute() to return 100")
        .unwrap();

    // Rebase: upstream prepended a comment, feature added+modified a function.
    // These are independent changes → no conflict expected.
    repo.git(&["rebase", &default_branch])
        .expect("rebase should succeed without conflicts");

    let sha_b = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // B′: hunk-based path. B's diff (A→B) replaces `return 42` with `return 100`.
    // `return 100` IS in original_head_line_to_author (feature tip content).
    // The hunk content-map lookup stamps it as AI.
    let note_b = repo
        .read_authorship_note(&sha_b)
        .expect("B′ must have a note: `return 100` is in the original-HEAD content map");
    assert!(!note_b.is_empty(), "B′ note must not be empty");

    // HEAD (= B′) must show compute() as AI-attributed.
    compute.assert_lines_and_blame(crate::lines![
        "// module",
        "fn base() {}",
        "fn compute() -> u32 { return 100; }".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_rebase_slow_path_line_attribution_is_correct,
    test_rebase_hunk_path_does_not_drop_ai_attribution_for_new_lines,
    test_rebase_second_commit_note_attributes_its_own_ai_lines,
    test_rebase_attribution_loss_compounds_across_three_commits,
    test_rebase_same_line_overwritten_by_consecutive_commits,
);
