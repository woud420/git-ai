use super::{ExpectedLineExt, TestRepo, files_in_note};

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
    test_rebase_empty_file_does_not_panic_or_pollute_attribution,
    test_rebase_conflict_on_ai_file_preserves_note,
    test_rebase_metadata_only_notes_survive_slow_path,
    test_rebase_mixed_ai_and_human_commits_all_retain_notes_after_slow_path,
);
