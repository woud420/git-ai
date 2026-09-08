use super::{
    ExpectedLineExt, TestRepo, assert_blame_at_commit, assert_note_base_commit_matches,
    assert_note_files_exact, assert_note_no_forbidden_files, fs, get_commit_chain,
};

/// Test: Human resolves conflict by replacing ALL AI lines with completely
/// different content.  After rebase, the conflict commit should have NO note
/// (commit_has_attestations=false → else branch returns None).
/// Subsequent AI commits should be unaffected.
#[test]
fn test_human_conflict_resolves_all_ai_lines_replaced() {
    let repo = TestRepo::new();

    // Base: compute.py with one human line
    repo.commit_untracked_file("compute.py", "result = 0\n", "Initial: result=0");
    let main_branch = repo.current_branch();

    // Main: change result to 1 (forces slow path on feature)
    repo.commit_untracked_file("compute.py", "result = 1\n", "main: set result=1");
    repo.commit_untracked_file("main_extra.py", "# main extra\n", "main: add extra file");

    // Feature from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~2"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI sets result=2 — WILL CONFLICT with main's result=1 (base=0)
    let mut compute = repo.filename("compute.py");
    compute.set_contents(crate::lines!["result = 2".ai(),]);
    repo.stage_all_and_commit("feat: C1 AI sets result=2")
        .unwrap();

    // C2: AI adds a separate file (unrelated to conflict)
    let mut module_b = repo.filename("module_b.py");
    module_b.set_contents(crate::lines![
        "class ModuleB:".ai(),
        "    def run(self): return 'b'".ai(),
        "    def name(self): return 'module_b'".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add ModuleB").unwrap();

    // C3: AI adds another file
    let mut module_c = repo.filename("module_c.py");
    module_c.set_contents(crate::lines![
        "class ModuleC:".ai(),
        "    def run(self): return 'c'".ai(),
        "    def name(self): return 'module_c'".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add ModuleC").unwrap();

    // Rebase: C1 conflicts on compute.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on compute.py at C1"
    );

    // Human resolves by writing COMPLETELY DIFFERENT content — no AI lines survive.
    // Base had result=0, feature had result=2, main had result=1.
    // Human writes result=42 with an extra human comment — none of these lines
    // match original AI content, so diff_based_line_attribution_transfer produces
    // only Replace ops → commit_has_attestations = false.
    //
    // However, the original commit DID have an AI authorship note.  Rather than
    // silently dropping provenance, the slow-path fallback remaps the original note
    // to the rebased commit.  The attestation line numbers may be stale but the AI
    // authorship record is preserved.
    fs::write(
        repo.path().join("compute.py"),
        "# human resolved\nresult = 42\n",
    )
    .unwrap();
    repo.git(&["add", "compute.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should succeed after C1 resolution");

    let chain = get_commit_chain(&repo, 3);
    // chain[0]=C1', chain[1]=C2', chain[2]=C3'

    // C1': human fully replaced all AI lines during resolution. Content-based mapping
    // finds no matching lines, so the note has no file attestations. The note itself
    // is preserved (metadata) but compute.py has no attributed lines.
    let c1_note = repo.read_authorship_note(&chain[0]);
    assert!(
        c1_note.is_some(),
        "C1 original had AI note: note metadata should be preserved after rewrite",
    );
    assert_note_files_exact(&repo, &chain[0], "c1_files", &[]);

    // C2': module_b.py — AI, untouched by conflict — note must exist with correct attribution
    assert_note_base_commit_matches(&repo, &chain[1], "c2");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["module_b.py"]);
    assert_note_no_forbidden_files(&repo, &chain[1], "c2_no_compute", &["compute.py"]);
    assert_blame_at_commit(
        &repo,
        &chain[1],
        "module_b.py",
        "c2_blame",
        &[
            ("class ModuleB:", true),
            ("def run(self): return 'b'", true),
            ("def name(self): return 'module_b'", true),
        ],
    );

    // C3': module_c.py — AI, untouched by conflict
    assert_note_base_commit_matches(&repo, &chain[2], "c3");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["module_c.py"]);
    assert_note_no_forbidden_files(&repo, &chain[2], "c3_no_compute", &["compute.py"]);
    assert_blame_at_commit(
        &repo,
        &chain[2],
        "module_c.py",
        "c3_blame",
        &[
            ("class ModuleC:", true),
            ("def run(self): return 'c'", true),
            ("def name(self): return 'module_c'", true),
        ],
    );
}

/// Regression test for #1079: when the ONLY AI-tracked file is the conflict file,
/// and the human resolves with completely different content, the original authorship
/// note must still be remapped to the rebased commit.  Before this fix the slow path
/// produced no note (content-diff found no matching AI lines) and the metadata-only
/// remap skipped notes with real attestations, silently losing provenance.
#[test]
fn test_human_conflict_ai_file_is_conflict_file_note_preserved() {
    let repo = TestRepo::new();

    // Initial: ai_file.py with one human line
    repo.commit_untracked_file("ai_file.py", "original line\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: change ai_file.py → will conflict with feature
    repo.commit_untracked_file(
        "ai_file.py",
        "upstream changed line\n",
        "main: modify ai_file",
    );

    // Feature branch from initial commit
    let base_sha = repo
        .git(&["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI modifies ai_file.py — this is the ONLY commit on feature, and the
    // ONLY file that has AI attribution.  It will conflict with main.
    let mut ai_file = repo.filename("ai_file.py");
    ai_file.set_contents(crate::lines!["ai modified line".ai()]);
    repo.stage_all_and_commit("feat: AI edits ai_file.py")
        .unwrap();

    // Verify note exists before rebase
    let pre_rebase_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let pre_note = repo.read_authorship_note(&pre_rebase_sha);
    assert!(
        pre_note.is_some(),
        "AI commit should have a note before rebase"
    );

    // Rebase onto main — conflict on ai_file.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on ai_file.py"
    );

    // Human resolves with completely different content (no AI lines survive).
    fs::write(repo.path().join("ai_file.py"), "human resolved content\n").unwrap();
    repo.git(&["add", "ai_file.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should succeed");

    let chain = get_commit_chain(&repo, 1);

    // The rebased commit still has a note (metadata preserved) but ai_file.py
    // has no attributed lines since human resolution replaced all AI content.
    let post_note = repo.read_authorship_note(&chain[0]);
    assert!(
        post_note.is_some(),
        "Note metadata should survive conflict rebase even when content doesn't match"
    );
    assert_note_files_exact(&repo, &chain[0], "c1_files", &[]);
}

/// Regression test for #1079: three AI commits on a feature branch; the second
/// commit's file conflicts with upstream.  After human conflict resolution and
/// `rebase --continue`, ALL three rebased commits must retain their authorship
/// notes.  Before the fix, the conflict commit's note was lost (content-diff
/// produced nothing for the manually resolved file and the fallback remap was
/// too narrow).
#[test]
fn test_human_conflict_multicommit_chain_middle_conflict_all_notes_preserved() {
    let repo = TestRepo::new();

    // Initial: shared.py (will conflict) + base.txt
    repo.commit_untracked_file("shared.py", "base content\n", "Initial commit");
    repo.commit_untracked_file("base.txt", "base\n", "Add base.txt");
    let main_branch = repo.current_branch();

    // Feature branch from initial commits
    let base_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates file_a.py (no conflict)
    let mut file_a = repo.filename("file_a.py");
    file_a.set_contents(crate::lines!["def ai_func_a(): pass".ai()]);
    repo.stage_all_and_commit("feat: AI creates file_a.py")
        .unwrap();

    // C2: AI modifies shared.py (WILL conflict with upstream)
    let mut shared = repo.filename("shared.py");
    shared.set_contents(crate::lines!["ai version of shared".ai()]);
    repo.stage_all_and_commit("feat: AI modifies shared.py")
        .unwrap();

    // C3: AI creates file_c.py (no conflict)
    let mut file_c = repo.filename("file_c.py");
    file_c.set_contents(crate::lines!["def ai_func_c(): pass".ai()]);
    repo.stage_all_and_commit("feat: AI creates file_c.py")
        .unwrap();

    // Verify all 3 commits have notes before rebase
    let chain_pre = get_commit_chain(&repo, 3);
    for (i, sha) in chain_pre.iter().enumerate() {
        assert!(
            repo.read_authorship_note(sha).is_some(),
            "pre-rebase commit {} (C{}) must have a note",
            &sha[..8],
            i + 1
        );
    }

    // Upstream: change shared.py to create conflict
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "shared.py",
        "upstream version of shared\n",
        "main: modify shared.py",
    );

    // Rebase feature onto main — C2 will conflict on shared.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on shared.py (C2 vs upstream)"
    );

    // Human resolves conflict with different content
    fs::write(repo.path().join("shared.py"), "human resolved shared\n").unwrap();
    repo.git(&["add", "shared.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .expect("rebase --continue should succeed");

    // All 3 rebased commits must have notes
    let chain = get_commit_chain(&repo, 3);

    // C1': file_a.py — AI, no conflict
    let note_c1 = repo.read_authorship_note(&chain[0]);
    assert!(
        note_c1.is_some(),
        "C1' (file_a.py, no conflict) must retain authorship note after conflict rebase (issue #1079)"
    );
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["file_a.py"]);

    // C2': shared.py — AI, conflict resolved by human with completely different content.
    // Content-based mapping finds no matching lines, so no file attestations remain.
    let note_c2 = repo.read_authorship_note(&chain[1]);
    assert!(
        note_c2.is_some(),
        "C2' note metadata should survive conflict rebase"
    );
    assert_note_files_exact(&repo, &chain[1], "c2_files", &[]);

    // C3': file_c.py — AI, no conflict
    let note_c3 = repo.read_authorship_note(&chain[2]);
    assert!(
        note_c3.is_some(),
        "C3' (file_c.py, no conflict) must retain authorship note after conflict rebase (issue #1079)"
    );
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["file_c.py"]);
}

crate::reuse_tests_in_worktree!(
    test_human_conflict_resolves_all_ai_lines_replaced,
    test_human_conflict_ai_file_is_conflict_file_note_preserved,
    test_human_conflict_multicommit_chain_middle_conflict_all_notes_preserved,
);
