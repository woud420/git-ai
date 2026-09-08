use super::{ExpectedLineExt, TestRepo, files_in_note, total_accepted_lines};

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

crate::reuse_tests_in_worktree!(
    test_rebase_future_file_does_not_leak_into_earlier_commit_note,
    test_rebase_intermediate_commit_accepted_lines_not_inflated,
    test_rebase_three_commits_no_future_file_leakage,
    test_rebase_deleted_file_does_not_persist_in_later_notes,
);
