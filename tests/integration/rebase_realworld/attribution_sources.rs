use super::*;

/// Verify that the working-log fallback path is the **sole** source of attribution
/// when an AI conflict resolution writes *different* content than the original commit.
///
/// Scenario:
///   - C1 writes `TIMEOUT = 30` as an AI line.
///   - Main changes the same constant to `TIMEOUT = 60` → rebase conflict.
///   - AI resolves by setting `TIMEOUT = 45` (a compromise — different from both sides).
///   - `set_contents` records a working-log checkpoint for the resolved value.
///   - Content-diff compares original (`= 30`) with resolved (`= 45`) → Replace → no match.
///   - The working-log fallback must fire and attribute `= 45` as AI.
///
/// Regression: if `build_note_from_conflict_wl` were removed, C1' would have no note.
#[test]
fn test_conflict_working_log_is_sole_attribution_source() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("config.py", "TIMEOUT = 10\nRETRIES = 3\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: changes TIMEOUT → will conflict
    repo.commit_untracked_file(
        "config.py",
        "TIMEOUT = 60\nRETRIES = 3\n",
        "main: increase timeout to 60",
    );
    repo.commit_untracked_file(
        "logging.py",
        "import logging\nlogging.basicConfig(level=logging.INFO)\n",
        "main: add logging config",
    );
    repo.commit_untracked_file(
        "metrics.py",
        "class Metrics:\n    pass\n",
        "main: add metrics stub",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~3"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI sets TIMEOUT = 30 — WILL CONFLICT with main's = 60
    let mut cfg = repo.filename("config.py");
    cfg.set_contents(crate::lines!["TIMEOUT = 30".ai(), "RETRIES = 3",]);
    fs::write(repo.path().join("config.py"), "TIMEOUT = 30\nRETRIES = 3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "config.py"])
        .unwrap();
    repo.stage_all_and_commit("feat: C1 AI sets TIMEOUT=30")
        .unwrap();

    // C2: AI adds a helper (conflict-free)
    let mut helper = repo.filename("helpers.py");
    helper.set_contents(crate::lines![
        "def retry(fn, n=3):".ai(),
        "    for i in range(n):".ai(),
        "        try: return fn()".ai(),
        "        except Exception:".ai(),
        "            if i == n - 1: raise".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add retry helper")
        .unwrap();

    // Rebase — C1 conflicts on config.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on config.py at C1"
    );

    // AI resolves: picks 45 as a compromise.  Content differs from original (30) → content-diff
    // cannot carry attribution.  ONLY the working-log checkpoint can produce the note.
    cfg.set_contents(crate::lines!["TIMEOUT = 45".ai(), "RETRIES = 3",]);
    fs::write(repo.path().join("config.py"), "TIMEOUT = 45\nRETRIES = 3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "config.py"])
        .unwrap();
    repo.git(&["add", "config.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 2);

    // C1': config.py only — note MUST exist (working-log fallback fired)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["config.py"]);
    // 1 AI line: TIMEOUT = 45 — accepted_lines must be 1 (not 0).
    // If build_note_from_conflict_wl hard-codes accepted_lines=0, this assertion fails.
    assert_accepted_lines_exact(&repo, &chain[0], "c1_accepted_lines", 1);
    // The resolved value (45) must be AI-attributed, not human.
    // This can only be true if build_note_from_conflict_wl contributed the note.
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "config.py",
        "c1_blame",
        &[("TIMEOUT = 45", true), ("RETRIES = 3", false)],
    );

    // C2': helpers.py only (unaffected by conflict)
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["helpers.py"]);
}

/// Verify that the content-diff path wins when it produces AI attribution, even when
/// a working-log checkpoint also exists for the same commit.
///
/// Scenario:
///   - C1 writes `MAX_RETRIES = 5` as an AI line.
///   - Main changes the same constant to `MAX_RETRIES = 10` → conflict.
///   - AI resolves by keeping the ORIGINAL value exactly: `MAX_RETRIES = 5`.
///   - `set_contents` records a working-log checkpoint.
///   - Content-diff sees `MAX_RETRIES = 5` (original) == `MAX_RETRIES = 5` (resolved) → Equal.
///   - `commit_has_attestations = true` → content-diff path wins; working-log is not consulted.
///   - Result: C1' note attributes `MAX_RETRIES = 5` as AI regardless of path.
#[test]
fn test_conflict_content_diff_wins_over_working_log() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "settings.py",
        "MAX_RETRIES = 3\nTIMEOUT = 10\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: changes MAX_RETRIES → will conflict
    repo.commit_untracked_file(
        "settings.py",
        "MAX_RETRIES = 10\nTIMEOUT = 10\n",
        "main: bump max retries",
    );
    repo.commit_untracked_file(
        "app.py",
        "from settings import MAX_RETRIES\n",
        "main: import settings",
    );
    repo.commit_untracked_file("server.py", "import http.server\n", "main: add server stub");

    let base_sha = repo
        .git(&["rev-parse", "HEAD~3"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI sets MAX_RETRIES = 5 — WILL CONFLICT with main's = 10
    let mut sett = repo.filename("settings.py");
    sett.set_contents(crate::lines!["MAX_RETRIES = 5".ai(), "TIMEOUT = 10",]);
    fs::write(
        repo.path().join("settings.py"),
        "MAX_RETRIES = 5\nTIMEOUT = 10\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "settings.py"])
        .unwrap();
    repo.stage_all_and_commit("feat: C1 AI sets MAX_RETRIES=5")
        .unwrap();

    // C2: AI adds a validator (conflict-free)
    let mut validator = repo.filename("validator.py");
    validator.set_contents(crate::lines![
        "def validate_retries(n: int) -> bool:".ai(),
        "    return isinstance(n, int) and 1 <= n <= 100".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add validator").unwrap();

    // Rebase — C1 conflicts on settings.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on settings.py at C1"
    );

    // AI resolves by keeping the ORIGINAL AI value exactly.
    // Content-diff: original `= 5` == resolved `= 5` → Equal → attribution carried.
    // Also creates a working-log checkpoint via set_contents.
    // The content-diff path fires first (commit_has_attestations=true) and wins.
    sett.set_contents(crate::lines!["MAX_RETRIES = 5".ai(), "TIMEOUT = 10",]);
    fs::write(
        repo.path().join("settings.py"),
        "MAX_RETRIES = 5\nTIMEOUT = 10\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "settings.py"])
        .unwrap();
    repo.git(&["add", "settings.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 2);

    // C1': settings.py — note exists because content-diff matched MAX_RETRIES = 5
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["settings.py"]);
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "settings.py",
        "c1_blame",
        &[("MAX_RETRIES = 5", true), ("TIMEOUT = 10", false)],
    );
    assert_accepted_lines_exact(&repo, &chain[0], "c1_accepted", 1);

    // C2': validator.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["validator.py"]);
}

crate::reuse_tests_in_worktree!(
    test_conflict_working_log_is_sole_attribution_source,
    test_conflict_content_diff_wins_over_working_log,
);
