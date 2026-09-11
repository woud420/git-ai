/// Tests for intermediate-commit note integrity after rebase.
///
/// ## The Bug
///
/// The old rebase authorship rewriter had a
/// slow-path processing loop that seeds `cached_file_attestation_text` and
/// `existing_files` from the **full cumulative state of the last pre-rebase commit**
/// (all commits in the chain combined). When it writes the note for an *intermediate*
/// new commit K, it emits ALL entries in `cached_file_attestation_text` that appear in
/// `existing_files`, which includes files introduced by commits K+1, K+2, … (future
/// commits).
///
/// Concrete example from PR #967 (5-commit chain):
///   - Commit f70ab45e (early – daemon changes only): note shows `revert_hooks.rs`
///     attributed, but revert_hooks.rs was first introduced in a LATER commit
///     (f1fdede4). Every intermediate commit ended up with the same large set of
///     file attestations as the tip commit, and `accepted_lines` counts became
///     non-monotonic (earlier commits showed higher counts than later ones).
///
/// ## When the slow path fires
///
/// The fast path (`try_fast_path_rebase_note_remap_cached`) just copies original
/// notes verbatim (only updating `base_commit_sha`) and is correct. It only fires
/// when the AI-touched file blobs are *identical* between old and new commits. If
/// *any* tracked file's blob changes after rebasing (e.g. the upstream prepended a
/// header to a file the feature also modifies), the fast path is skipped and the
/// buggy slow path runs.
///
/// ## Setup pattern used to force the slow path
///
/// Each test creates a `shared.rs` file (with a proper trailing newline, committed via
/// `git_og` to avoid the no-trailing-newline issue with `set_contents`). The upstream
/// branch prepends a header to `shared.rs`. The feature branch then APPENDS to
/// `shared.rs` via `set_contents` (which writes content without a trailing newline –
/// that's fine because git can merge "prepend on upstream" with "append on feature"
/// even when they have different trailing-newline styles).
///
/// After rebasing, the shared.rs blob in each feature commit differs from its
/// pre-rebase counterpart, so `tracked_paths_match_for_commit_pairs` returns false
/// → fast path is bypassed → the buggy slow path runs.
///
/// ## Expected vs broken behaviour
///
/// | Commit | Expected note files                | Broken note files (current)          |
/// |--------|------------------------------------|--------------------------------------|
/// | A′     | shared.rs + module_a.rs            | shared.rs + module_a.rs + module_b.rs ← LEAK |
/// | B′     | shared.rs + module_a.rs + module_b.rs | correct (it is the tip)           |
///
/// These tests are intentionally written to **FAIL** with the current (buggy) code
/// and to **PASS** once the bug is fixed.
use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::session_line_count;
use git_ai::model::authorship_log_serialization::AuthorshipLog;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn total_accepted_lines(note: &str) -> u32 {
    let log = AuthorshipLog::deserialize_from_string(note)
        .expect("should parse authorship note as AuthorshipLog");
    // Count AI lines from attestations where hash starts with "s_" (sessions)
    session_line_count(&log)
}

fn files_in_note(note: &str) -> Vec<String> {
    let log = AuthorshipLog::deserialize_from_string(note)
        .expect("should parse authorship note as AuthorshipLog");
    log.attestations
        .iter()
        .map(|a| a.file_path.clone())
        .collect()
}

mod line_attribution;

// ---------------------------------------------------------------------------
// Test 1: future-file attribution must not leak into earlier commit notes
// ---------------------------------------------------------------------------

/// After a rebase where the slow path fires (upstream prepended a line to
/// shared.rs, diverging blobs), commit A′'s note must NOT reference module_b.rs,
/// which was only introduced by commit B (a later commit).
///
/// Broken: the slow path seeds `cached_file_attestation_text` + `existing_files`
/// from the final pre-rebase state. module_b.rs is in that state (added by B),
/// so it leaks into every intermediate commit's note including A′.
#[test]
fn test_rebase_future_file_does_not_leak_into_earlier_commit_note() {
    let repo = TestRepo::new();

    // Initial commit: shared.rs with a proper trailing newline (via git_og).
    // Feature branch will APPEND lines; upstream will PREPEND.
    // 3-way merge: prepend (upstream) + append (feature) = non-conflicting.
    repo.commit_untracked_file("shared.rs", "fn original() {}", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream PREPENDS a header line to shared.rs.
    // After rebasing, every feature commit that touches shared.rs will have a
    // different blob OID → fast path cannot fire → slow path runs.
    repo.commit_untracked_file(
        "shared.rs",
        "// upstream header\nfn original() {}",
        "Upstream: prepend header to shared.rs",
    );

    // Feature branch starts from BEFORE the upstream commit.
    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // Commit A: appends AI lines to shared.rs + creates module_a.rs.
    // module_b.rs does NOT exist at this point.
    let mut shared = repo.filename("shared.rs");
    shared.set_contents(crate::lines![
        "fn original() {}",
        "fn a1() {}".ai(),
        "fn a2() {}".ai()
    ]);
    let mut module_a = repo.filename("module_a.rs");
    module_a.set_contents(crate::lines!["fn ma() {}".ai()]);
    repo.stage_all_and_commit("Commit A: shared (append) + module_a.rs")
        .unwrap();

    // Commit B: appends more AI lines to shared.rs + creates module_b.rs.
    shared.set_contents(crate::lines![
        "fn original() {}",
        "fn a1() {}".ai(),
        "fn a2() {}".ai(),
        "fn b1() {}".ai(),
        "fn b2() {}".ai()
    ]);
    let mut module_b = repo.filename("module_b.rs");
    module_b.set_contents(crate::lines!["fn mb1() {}".ai(), "fn mb2() {}".ai()]);
    repo.stage_all_and_commit("Commit B: shared (append) + module_b.rs")
        .unwrap();

    // Rebase feature onto the advanced main branch (non-conflicting).
    repo.git(&["rebase", &default_branch])
        .expect("rebase should succeed without conflicts");

    let new_sha_b = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let new_sha_a = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();

    let note_a = repo
        .read_authorship_note(&new_sha_a)
        .expect("commit A′ must have an authorship note after rebase");
    let note_b = repo
        .read_authorship_note(&new_sha_b)
        .expect("commit B′ must have an authorship note after rebase");

    let files_a = files_in_note(&note_a);
    let files_b = files_in_note(&note_b);

    // -----------------------------------------------------------------------
    // Core assertion: module_b.rs was introduced in commit B (AFTER commit A).
    // Commit A′'s note must NOT reference module_b.rs.
    // With the slow-path bug, the cache is pre-seeded from the final pre-rebase
    // state which already includes module_b.rs → it leaks into A′'s note.
    // -----------------------------------------------------------------------
    assert!(
        !files_a.iter().any(|f| f.contains("module_b")),
        "REBASE NOTE CORRUPTION (slow-path future-file leak): \
         Commit A′'s note contains 'module_b.rs', but module_b.rs was only \
         introduced in commit B (a later commit). \
         The slow path seeds cached_file_attestation_text from the full \
         pre-rebase HEAD state, causing future files to appear in earlier \
         commit notes. Files found in A′'s note: {:?}",
        files_a
    );

    // Sanity: A′ should reference the files A actually introduced.
    assert!(
        files_a
            .iter()
            .any(|f| f.contains("module_a") || f.contains("shared")),
        "Commit A′'s note should contain module_a.rs or shared.rs, \
         but found: {:?}",
        files_a
    );

    // Sanity: B′ (tip) must include module_b.rs.
    assert!(
        files_b.iter().any(|f| f.contains("module_b")),
        "Commit B′'s note should contain module_b.rs, but found: {:?}",
        files_b
    );
}

// ---------------------------------------------------------------------------
// Test 2: accepted_lines must not be inflated for intermediate commits
// ---------------------------------------------------------------------------

/// Two-commit feature branch where both commits append to a shared file that
/// the upstream prepended to (forcing the slow path without conflicts).
///
/// Commit 1 adds exactly 10 AI lines. Commit 2 adds 10 more. After rebase,
/// commit 1′'s `accepted_lines` should reflect only its own ~10 lines, not
/// the full-chain total of ~20.
///
/// Broken: the slow path writes the full-chain `accepted_lines` to every
/// intermediate commit because `current_attributions` starts at the final
/// pre-rebase state and is never rewound to the per-commit checkpoint.
#[test]
fn test_rebase_intermediate_commit_accepted_lines_not_inflated() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("impl.rs", "fn base() {}", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream prepends to impl.rs (diverges blobs → forces slow path).
    repo.commit_untracked_file(
        "impl.rs",
        "// upstream header\nfn base() {}",
        "Upstream: prepend to impl.rs",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    let mut shared = repo.filename("impl.rs");

    // Commit 1: appends exactly 10 AI lines to impl.rs.
    shared.set_contents(crate::lines![
        "fn base() {}",
        "fn c01() {}".ai(),
        "fn c02() {}".ai(),
        "fn c03() {}".ai(),
        "fn c04() {}".ai(),
        "fn c05() {}".ai(),
        "fn c06() {}".ai(),
        "fn c07() {}".ai(),
        "fn c08() {}".ai(),
        "fn c09() {}".ai(),
        "fn c10() {}".ai()
    ]);
    repo.stage_all_and_commit("Commit 1: 10 AI lines appended to impl.rs")
        .unwrap();

    // Commit 2: appends 10 more AI lines to impl.rs.
    shared.set_contents(crate::lines![
        "fn base() {}",
        "fn c01() {}".ai(),
        "fn c02() {}".ai(),
        "fn c03() {}".ai(),
        "fn c04() {}".ai(),
        "fn c05() {}".ai(),
        "fn c06() {}".ai(),
        "fn c07() {}".ai(),
        "fn c08() {}".ai(),
        "fn c09() {}".ai(),
        "fn c10() {}".ai(),
        "fn c11() {}".ai(),
        "fn c12() {}".ai(),
        "fn c13() {}".ai(),
        "fn c14() {}".ai(),
        "fn c15() {}".ai(),
        "fn c16() {}".ai(),
        "fn c17() {}".ai(),
        "fn c18() {}".ai(),
        "fn c19() {}".ai(),
        "fn c20() {}".ai()
    ]);
    repo.stage_all_and_commit("Commit 2: 10 more AI lines appended to impl.rs")
        .unwrap();

    repo.git(&["rebase", &default_branch])
        .expect("rebase should succeed without conflicts");

    let new_sha2 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let new_sha1 = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();

    let note1 = repo
        .read_authorship_note(&new_sha1)
        .expect("commit 1′ should have an authorship note");
    let note2 = repo
        .read_authorship_note(&new_sha2)
        .expect("commit 2′ should have an authorship note");

    let lines1 = total_accepted_lines(&note1);
    let lines2 = total_accepted_lines(&note2);

    // Each commit's note attributes the AI lines that IT introduced (per the diff from parent).
    // Commit 1 introduced c01-c10 (10 AI lines) over base. Due to trailing-newline diff
    // handling, the last line of base (fn base()) also appears in committed_hunks but has
    // no AI attribution, so only 10 AI lines survive.
    // Commit 2 introduced c11-c20 (10 more AI lines) over commit 1. Similarly ~10-11 lines.
    // The key invariant: commit 1′ must NOT show 20 (that would mean future-commit leakage).
    assert_eq!(
        lines1, 10,
        "REBASE NOTE CORRUPTION: commit 1′ should report exactly 10 AI lines (file state at commit 1), got {}. If > 10, the slow path is leaking future commit lines.",
        lines1
    );
    assert_eq!(
        lines2, 11,
        "commit 2′ should report 11 AI lines (c10-c20 in committed_hunks due to trailing newline), got {}.",
        lines2
    );
}

// ---------------------------------------------------------------------------
// Test 3: three-commit chain — no future-file leakage (slow path forced)
// ---------------------------------------------------------------------------

/// Three-commit feature branch, each appending to a shared file that upstream
/// prepended to (forcing slow path). Each commit also adds its own unique file.
///
/// After rebase:
///   - A′ note: must NOT contain unit_b.rs or unit_c.rs (future files)
///   - B′ note: must NOT contain unit_c.rs (future file)
///   - C′ note: tip commit, no future-leak concern
#[test]
fn test_rebase_three_commits_no_future_file_leakage() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("core.rs", "fn core_base() {}", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream prepends to core.rs → forces slow path.
    repo.commit_untracked_file(
        "core.rs",
        "// upstream\nfn core_base() {}",
        "Upstream: prepend to core.rs",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    let mut shared = repo.filename("core.rs");

    // Commit A: appends to core.rs + adds unit_a.rs.
    shared.set_contents(crate::lines!["fn core_base() {}", "fn core_a() {}".ai()]);
    let mut unit_a = repo.filename("unit_a.rs");
    unit_a.set_contents(crate::lines!["fn ua() {}".ai()]);
    repo.stage_all_and_commit("Commit A: core + unit_a")
        .unwrap();

    // Commit B: appends to core.rs + adds unit_b.rs.
    shared.set_contents(crate::lines![
        "fn core_base() {}",
        "fn core_a() {}".ai(),
        "fn core_b() {}".ai()
    ]);
    let mut unit_b = repo.filename("unit_b.rs");
    unit_b.set_contents(crate::lines!["fn ub() {}".ai()]);
    repo.stage_all_and_commit("Commit B: core + unit_b")
        .unwrap();

    // Commit C: appends to core.rs + adds unit_c.rs.
    shared.set_contents(crate::lines![
        "fn core_base() {}",
        "fn core_a() {}".ai(),
        "fn core_b() {}".ai(),
        "fn core_c() {}".ai()
    ]);
    let mut unit_c = repo.filename("unit_c.rs");
    unit_c.set_contents(crate::lines!["fn uc() {}".ai()]);
    repo.stage_all_and_commit("Commit C: core + unit_c")
        .unwrap();

    repo.git(&["rebase", &default_branch])
        .expect("rebase should succeed without conflicts");

    let _sha_c = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let sha_b = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    let sha_a = repo
        .git(&["rev-parse", "HEAD~2"])
        .unwrap()
        .trim()
        .to_string();

    let note_a = repo
        .read_authorship_note(&sha_a)
        .expect("commit A′ should have note");
    let note_b = repo
        .read_authorship_note(&sha_b)
        .expect("commit B′ should have note");

    let files_a = files_in_note(&note_a);
    let files_b = files_in_note(&note_b);

    // A′ must not reference unit_b.rs or unit_c.rs (future files).
    assert!(
        !files_a.iter().any(|f| f.contains("unit_b")),
        "REBASE NOTE CORRUPTION: commit A′'s note contains 'unit_b.rs', \
         which was only introduced in commit B (a later commit). \
         Files in A′: {:?}",
        files_a
    );
    assert!(
        !files_a.iter().any(|f| f.contains("unit_c")),
        "REBASE NOTE CORRUPTION: commit A′'s note contains 'unit_c.rs', \
         which was only introduced in commit C (a later commit). \
         Files in A′: {:?}",
        files_a
    );

    // B′ must not reference unit_c.rs (future file relative to B).
    assert!(
        !files_b.iter().any(|f| f.contains("unit_c")),
        "REBASE NOTE CORRUPTION: commit B′'s note contains 'unit_c.rs', \
         which was only introduced in commit C (a later commit). \
         Files in B′: {:?}",
        files_b
    );

    // Sanity: A′ should reference what A actually introduced.
    assert!(
        files_a
            .iter()
            .any(|f| f.contains("unit_a") || f.contains("core")),
        "Commit A′ should reference unit_a.rs or core.rs, but found: {:?}",
        files_a
    );
}

// ---------------------------------------------------------------------------
// Test 4: deleted file must not reappear in later commit notes (slow path)
// ---------------------------------------------------------------------------

/// Commit A appends to shared file + adds temp.rs.
/// Commit B appends to shared file + deletes temp.rs + adds final.rs.
/// Commit C appends to shared file + adds extra.rs.
/// Upstream prepends to shared file (forces slow path).
///
/// After rebase:
///   - B′ note: must NOT contain temp.rs (it was deleted in B)
///   - B′ note: must NOT contain extra.rs (introduced in future commit C)
///   - C′ note: must NOT contain temp.rs (deleted before C ever ran)
#[test]
fn test_rebase_deleted_file_does_not_persist_in_later_notes() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("engine.rs", "fn engine_base() {}", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream prepends to engine.rs → forces slow path.
    repo.commit_untracked_file(
        "engine.rs",
        "// upstream\nfn engine_base() {}",
        "Upstream: prepend to engine.rs",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    let mut engine = repo.filename("engine.rs");

    // Commit A: appends to engine.rs + adds temp.rs.
    engine.set_contents(crate::lines!["fn engine_base() {}", "fn eng_a() {}".ai()]);
    let mut temp = repo.filename("temp.rs");
    temp.set_contents(crate::lines!["fn tmp1() {}".ai(), "fn tmp2() {}".ai()]);
    repo.stage_all_and_commit("Commit A: engine + temp.rs")
        .unwrap();

    // Commit B: appends to engine.rs + removes temp.rs + adds final.rs.
    engine.set_contents(crate::lines![
        "fn engine_base() {}",
        "fn eng_a() {}".ai(),
        "fn eng_b() {}".ai()
    ]);
    repo.git(&["rm", "temp.rs"]).unwrap();
    let mut final_rs = repo.filename("final.rs");
    final_rs.set_contents(crate::lines!["fn fin() {}".ai()]);
    repo.stage_all_and_commit("Commit B: engine + rm temp.rs + final.rs")
        .unwrap();

    // Commit C: appends to engine.rs + adds extra.rs.
    engine.set_contents(crate::lines![
        "fn engine_base() {}",
        "fn eng_a() {}".ai(),
        "fn eng_b() {}".ai(),
        "fn eng_c() {}".ai()
    ]);
    let mut extra = repo.filename("extra.rs");
    extra.set_contents(crate::lines!["fn ex() {}".ai()]);
    repo.stage_all_and_commit("Commit C: engine + extra.rs")
        .unwrap();

    repo.git(&["rebase", &default_branch])
        .expect("rebase should succeed without conflicts");

    let sha_c = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let sha_b = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();

    let note_b = repo
        .read_authorship_note(&sha_b)
        .expect("commit B′ should have note");
    let note_c = repo
        .read_authorship_note(&sha_c)
        .expect("commit C′ should have note");

    let files_b = files_in_note(&note_b);
    let files_c = files_in_note(&note_c);

    // B′: temp.rs was deleted in B — must not appear.
    assert!(
        !files_b.iter().any(|f| f.contains("temp")),
        "REBASE NOTE CORRUPTION: commit B′ contains 'temp.rs', which was \
         deleted in commit B. files_b: {:?}",
        files_b
    );

    // B′: extra.rs was introduced in commit C (future) — must not appear.
    assert!(
        !files_b.iter().any(|f| f.contains("extra")),
        "REBASE NOTE CORRUPTION: commit B′ contains 'extra.rs', which was \
         only introduced in commit C (a later commit). files_b: {:?}",
        files_b
    );

    // C′: temp.rs was deleted in B (before C) — must not appear in C.
    assert!(
        !files_c.iter().any(|f| f.contains("temp")),
        "REBASE NOTE CORRUPTION: commit C′ contains 'temp.rs', which was \
         deleted in commit B (before C). files_c: {:?}",
        files_c
    );

    // Sanity: final.rs must appear in B′ (B introduced it).
    assert!(
        files_b.iter().any(|f| f.contains("final")),
        "Commit B′ should contain final.rs, but found: {:?}",
        files_b
    );
    // Per-commit-delta: C′ only contains files that C itself touched (engine.rs, extra.rs).
    // final.rs was introduced in B, not C, so it must NOT appear in C′.
    assert!(
        !files_c.iter().any(|f| f.contains("final")),
        "Commit C′ should NOT contain final.rs (per-commit-delta: C didn't touch it), \
         but found: {:?}",
        files_c
    );
}

// ---------------------------------------------------------------------------
// Test 10: empty file in slow path — no panic, no spurious attribution
// ---------------------------------------------------------------------------

/// When a commit introduces an empty file (0 bytes), the slow path must not panic
/// and must produce no AI attribution for that file.  A second non-empty AI file
/// in the same commit still receives correct attribution.
#[test]
fn test_rebase_empty_file_does_not_panic_or_pollute_attribution() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("existing.rs", "fn base() {}\n", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream prepends to existing.rs → forces slow path for the feature commits.
    repo.commit_untracked_file(
        "existing.rs",
        "// upstream header\nfn base() {}\n",
        "Upstream: prepend header",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    let mut existing = repo.filename("existing.rs");

    // Commit 1: AI modifies existing.rs AND creates an empty file.
    existing.set_contents(crate::lines!["fn base() {}", "fn ai_fn() {}".ai(),]);
    // Create an empty file alongside the AI change.
    std::fs::write(repo.path().join("empty.rs"), b"").unwrap();
    repo.git(&["add", "empty.rs"]).unwrap();
    repo.stage_all_and_commit("feat: AI adds ai_fn + empty placeholder")
        .unwrap();

    repo.git(&["rebase", &default_branch])
        .expect("rebase should succeed");

    let sha1 = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    let note = repo
        .read_authorship_note(&sha1)
        .expect("commit 1′ must have a note for existing.rs");

    // existing.rs must be attributed (AI line survived content-diff).
    let files: Vec<String> = files_in_note(&note);
    assert!(
        files.iter().any(|f| f.contains("existing")),
        "note must include existing.rs, got: {:?}",
        files
    );
    // empty.rs must NOT appear in the note — an empty file has no AI lines.
    assert!(
        !files.iter().any(|f| f.contains("empty")),
        "empty.rs must not appear in attribution note, got: {:?}",
        files
    );
}

// ---------------------------------------------------------------------------
// Issue #1079: conflict rebase — AI file IS the conflict file
// ---------------------------------------------------------------------------

/// When the ONLY AI-tracked file is the one that has a merge conflict, and the
/// human resolves the conflict manually (not through git-ai), the authorship note
/// must survive the rebase.  Before the fix:
///   1. Fast path fails (blobs differ due to conflict resolution)
///   2. Slow path content-diff finds no matching AI lines in the human-resolved content
///   3. The note is silently dropped (no fallback remap)
///
/// Fix: after the slow-path loop, remap the original note for any commit that
/// had a note but wasn't covered by the diff-based attribution transfer.
#[test]
fn test_rebase_conflict_on_ai_file_preserves_note() {
    let repo = TestRepo::new();

    // shared.rs with trailing newline for clean conflict detection.
    repo.commit_untracked_file("shared.rs", "fn original() {}", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream: completely different content for shared.rs → will conflict.
    repo.commit_untracked_file(
        "shared.rs",
        "fn upstream_version() {}",
        "Upstream: rewrite shared.rs",
    );

    // Feature branch from before upstream change.
    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // AI modifies shared.rs — the only commit, the only AI file.
    let mut shared = repo.filename("shared.rs");
    shared.set_contents(crate::lines!["fn ai_version() {}".ai()]);
    repo.stage_all_and_commit("feat: AI rewrites shared.rs")
        .unwrap();

    // Verify note exists before rebase.
    let pre_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert!(
        repo.read_authorship_note(&pre_sha).is_some(),
        "AI commit must have a note before rebase"
    );

    // Rebase → conflict on shared.rs.
    repo.git(&["checkout", "feature"]).unwrap();
    let result = repo.git(&["rebase", &default_branch]);
    assert!(result.is_err(), "rebase should conflict on shared.rs");

    // Human resolves with completely different content (no AI lines survive).
    std::fs::write(repo.path().join("shared.rs"), "fn human_resolved() {}\n").unwrap();
    repo.git(&["add", "shared.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should succeed");

    // Post-rebase: the note must still exist (remapped from original).
    let post_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let post_note = repo.read_authorship_note(&post_sha);
    assert!(
        post_note.is_some(),
        "AI authorship note must survive conflict rebase where the AI file IS the \
         conflict file (issue #1079). The original note should be remapped to the \
         rebased commit to preserve AI provenance."
    );
}

// ---------------------------------------------------------------------------
// Issue #1079: metadata-only notes must survive slow-path rebase
// ---------------------------------------------------------------------------

/// When a rebase forces the slow path (AI-tracked file blobs differ between
/// original and rebased commits), human-only commits that touch DIFFERENT files
/// than the AI-tracked files used to lose their notes.  The slow path only wrote
/// notes for commits whose diff-tree intersected the AI pathspecs, silently
/// dropping metadata-only notes.
///
/// Fix: after the slow-path loop, remap original metadata-only notes for any
/// commits not covered by the diff-based attribution transfer.
#[test]
fn test_rebase_metadata_only_notes_survive_slow_path() {
    let repo = TestRepo::new();

    // shared.rs with trailing newline via git_og for clean 3-way merge.
    repo.commit_untracked_file("shared.rs", "fn original() {}", "Initial commit");
    let default_branch = repo.current_branch();

    // Upstream prepends to shared.rs → forces slow path (blob differs after rebase).
    repo.commit_untracked_file(
        "shared.rs",
        "// upstream header\nfn original() {}",
        "Upstream: prepend header to shared.rs",
    );

    // Feature branch from before the upstream change.
    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // Commit A: AI modifies shared.rs (creates real attestation).
    let mut shared = repo.filename("shared.rs");
    shared.set_contents(crate::lines!["fn original() {}", "fn ai_added() {}".ai()]);
    repo.stage_all_and_commit("Commit A: AI changes shared.rs")
        .unwrap();

    // Commit B: Human adds a DIFFERENT file (metadata-only note, no AI pathspecs).
    let mut human_file = repo.filename("human_only.txt");
    human_file.set_contents(crate::lines!["human work"]);
    repo.stage_all_and_commit("Commit B: human-only change")
        .unwrap();
    let pre_rebase_human_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Verify human commit has a note before rebase.
    let pre_note = repo.read_authorship_note(&pre_rebase_human_sha);
    assert!(
        pre_note.is_some(),
        "human-only commit should have a metadata-only note before rebase"
    );

    // Rebase feature onto upstream (forces slow path because shared.rs blob differs).
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // Get post-rebase commit SHAs.
    let log_output = repo
        .git(&["log", "--oneline", &format!("{}..feature", default_branch)])
        .unwrap();
    let commit_count = log_output.trim().lines().count();
    assert_eq!(commit_count, 2, "should have 2 rebased commits");

    // Verify AI commit preserved its attestation.
    shared.assert_lines_and_blame(crate::lines![
        "// upstream header",
        "fn original() {}",
        "fn ai_added() {}".ai()
    ]);

    // Verify human-only commit still has a note after rebase (the fix for #1079).
    let post_rebase_human_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let post_note = repo.read_authorship_note(&post_rebase_human_sha);
    assert!(
        post_note.is_some(),
        "human-only commit must retain its metadata-only note after slow-path rebase (issue #1079)"
    );
}

/// Same as above but with 3 AI commits and 2 human-only commits interleaved,
/// ensuring all notes survive the slow path.
#[test]
fn test_rebase_mixed_ai_and_human_commits_all_retain_notes_after_slow_path() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("shared.rs", "fn original() {}", "Initial commit");
    let default_branch = repo.current_branch();

    repo.commit_untracked_file(
        "shared.rs",
        "// header\nfn original() {}",
        "Upstream: prepend",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // Commit 1: AI modifies shared.rs
    let mut shared = repo.filename("shared.rs");
    shared.set_contents(crate::lines!["fn original() {}", "fn ai1() {}".ai()]);
    repo.stage_all_and_commit("AI commit 1").unwrap();

    // Commit 2: Human adds file_a.txt
    let mut file_a = repo.filename("file_a.txt");
    file_a.set_contents(crate::lines!["human file a"]);
    repo.stage_all_and_commit("Human commit 2").unwrap();

    // Commit 3: AI adds module_b.rs
    let mut module_b = repo.filename("module_b.rs");
    module_b.set_contents(crate::lines!["fn b() {}".ai()]);
    repo.stage_all_and_commit("AI commit 3").unwrap();

    // Commit 4: Human adds file_c.txt
    let mut file_c = repo.filename("file_c.txt");
    file_c.set_contents(crate::lines!["human file c"]);
    repo.stage_all_and_commit("Human commit 4").unwrap();

    // Commit 5: AI appends to shared.rs
    shared.set_contents(crate::lines![
        "fn original() {}",
        "fn ai1() {}".ai(),
        "fn ai5() {}".ai()
    ]);
    repo.stage_all_and_commit("AI commit 5").unwrap();

    // Rebase
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // All 5 rebased commits must have notes.
    let log_output = repo
        .git(&[
            "log",
            "--format=%H",
            &format!("{}..feature", default_branch),
        ])
        .unwrap();
    let rebased_shas: Vec<&str> = log_output.trim().lines().collect();
    assert_eq!(rebased_shas.len(), 5, "should have 5 rebased commits");

    for sha in &rebased_shas {
        let note = repo.read_authorship_note(sha);
        assert!(
            note.is_some(),
            "rebased commit {} must have an authorship note after slow-path rebase (issue #1079)",
            &sha[..8]
        );
    }

    // Verify AI attribution survived.
    shared.assert_lines_and_blame(crate::lines![
        "// header",
        "fn original() {}",
        "fn ai1() {}".ai(),
        "fn ai5() {}".ai()
    ]);
    module_b.assert_lines_and_blame(crate::lines!["fn b() {}".ai()]);
}

crate::reuse_tests_in_worktree!(
    test_rebase_future_file_does_not_leak_into_earlier_commit_note,
    test_rebase_intermediate_commit_accepted_lines_not_inflated,
    test_rebase_three_commits_no_future_file_leakage,
    test_rebase_deleted_file_does_not_persist_in_later_notes,
    test_rebase_empty_file_does_not_panic_or_pollute_attribution,
    test_rebase_conflict_on_ai_file_preserves_note,
    test_rebase_metadata_only_notes_survive_slow_path,
    test_rebase_mixed_ai_and_human_commits_all_retain_notes_after_slow_path,
);
