use super::{
    ExpectedLineExt, TestRepo, assert_accepted_lines_exact, assert_blame_at_commit,
    assert_note_base_commit_matches, assert_note_files_exact, fs, get_commit_chain,
};

/// Test 10: Two conflicts — C2 (AI resolved) and C4 (human resolved).
/// Verifies that after two sequential conflicts in the same rebase,
/// AI attribution is tracked correctly: C2' gets AI config_a.py; C4' does NOT
/// get AI config_b.py (human resolved).
#[test]
fn test_conflict_mixed_ai_and_human_resolve_different_commits() {
    let repo = TestRepo::new();

    // Initial: config files with numeric values (0/10) so both sides can make
    // clearly conflicting changes. Using numbers avoids trailing-newline ambiguity
    // in git's merge and ensures non-empty rebased commits after resolution.
    repo.commit_untracked_file("config_a.py", "FLAG_A = 0\n", "Initial commit");
    repo.commit_untracked_file(
        "config_b.py",
        "FLAG_B = 0\nBATCH = 10\n",
        "Initial config_b",
    );
    let main_branch = repo.current_branch();

    // Main commits (human): set FLAG_A=1, FLAG_B=1/BATCH=50, then 3 more files
    repo.commit_untracked_file("config_a.py", "FLAG_A = 1\n", "main: set flag_a to 1");
    repo.commit_untracked_file(
        "config_b.py",
        "FLAG_B = 1\nBATCH = 50\n",
        "main: set flag_b and batch 50",
    );
    repo.commit_untracked_file(
        "app.py",
        "print('app started')\n",
        "main: add app entry point",
    );
    repo.commit_untracked_file(
        "db.py",
        "class Database: pass\n",
        "main: add database class",
    );
    repo.commit_untracked_file("cache.py", "class Cache: pass\n", "main: add cache class");

    // Feature branch from base (5 commits before main HEAD = the "Initial config_b" commit)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates module_a.py (10 AI lines)
    let mut module_a = repo.filename("module_a.py");
    module_a.set_contents(crate::lines![
        "class ModuleA:".ai(),
        "    def __init__(self, config):".ai(),
        "        self.config = config".ai(),
        "        self.flag = config.get('FLAG_A', 0)".ai(),
        "    def run(self):".ai(),
        "        if not self.flag: return".ai(),
        "        print('ModuleA running')".ai(),
        "    def status(self): return {'flag': self.flag}".ai(),
        "    def name(self): return 'module_a'".ai(),
        "    def version(self): return '1.0'".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add ModuleA").unwrap();

    // C2: AI changes FLAG_A to 2 — WILL CONFLICT with main's 1 (base=0, feature=2, main=1 → conflict)
    let mut config_a = repo.filename("config_a.py");
    config_a.set_contents(crate::lines!["FLAG_A = 2".ai(),]);
    repo.stage_all_and_commit("feat: C2 AI sets FLAG_A=2")
        .unwrap();

    // C3: AI creates module_c.py (10 AI lines)
    let mut module_c = repo.filename("module_c.py");
    module_c.set_contents(crate::lines![
        "class ModuleC:".ai(),
        "    def __init__(self, config):".ai(),
        "        self.config = config".ai(),
        "        self.batch = config.get('BATCH', 10)".ai(),
        "    def process(self, items):".ai(),
        "        batches = [items[i:i+self.batch] for i in range(0, len(items), self.batch)]".ai(),
        "        return [self._process_batch(b) for b in batches]".ai(),
        "    def _process_batch(self, batch): return batch".ai(),
        "    def name(self): return 'module_c'".ai(),
        "    def version(self): return '1.0'".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add ModuleC").unwrap();

    // C4: AI changes config_b.py — WILL CONFLICT on BATCH (feature=200 vs main=50)
    // FLAG_B: base=0, feature=1, main=1 → auto-merged (same)
    // BATCH: base=10, feature=200, main=50 → conflict
    let mut config_b = repo.filename("config_b.py");
    config_b.set_contents(crate::lines!["FLAG_B = 1".ai(), "BATCH = 200".ai(),]);
    repo.stage_all_and_commit("feat: C4 AI sets BATCH=200")
        .unwrap();

    // C5: AI creates module_e.py (10 AI lines)
    let mut module_e = repo.filename("module_e.py");
    module_e.set_contents(crate::lines![
        "class ModuleE:".ai(),
        "    def __init__(self, config):".ai(),
        "        self.config = config".ai(),
        "    def execute(self, task):".ai(),
        "        return {'task': task, 'done': True}".ai(),
        "    def cancel(self, task_id):".ai(),
        "        return {'task_id': task_id, 'cancelled': True}".ai(),
        "    def list_tasks(self): return []".ai(),
        "    def name(self): return 'module_e'".ai(),
        "    def version(self): return '1.0'".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add ModuleE").unwrap();

    // Rebase — C2 will conflict first on config_a.py (feature=2 vs main=1, base=0)
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on config_a.py at C2"
    );

    // AI resolves C2: keeps feature's value (FLAG_A = 2) → C2' is non-empty since parent has 1.
    // Use set_contents_no_stage to avoid accidentally staging config_b.py, then stage only config_a.py.
    let mut conflict_config_a = repo.filename("config_a.py");
    conflict_config_a.set_contents_no_stage(crate::lines!["FLAG_A = 2".ai(),]);
    repo.git(&["add", "config_a.py"]).unwrap();
    let continue_result =
        repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None);
    // C4 should now conflict on BATCH (200 vs 50, base=10)
    assert!(
        continue_result.is_err(),
        "rebase should conflict on config_b.py at C4"
    );

    // Human resolves C4: compromise value BATCH=75 → C4' is non-empty (parent has BATCH=50)
    fs::write(repo.path().join("config_b.py"), "FLAG_B = 1\nBATCH = 75\n").unwrap();
    repo.git(&["add", "config_b.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': module_a.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["module_a.py"]);

    // C2': config_a.py only (AI-resolved, keeps feature's FLAG_A=2)
    // The original C2 had "FLAG_A = 2\n" as AI; the resolution keeps the same content.
    // diff_based: old="FLAG_A = 2\n", new="FLAG_A = 2\n" → Equal → AI ✓
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["config_a.py"]);

    // blame at chain[1]: git blame says C2' introduced "FLAG_A = 2" (parent had FLAG_A=1) → AI
    assert_blame_at_commit(
        &repo,
        &chain[1],
        "config_a.py",
        "c2_blame_config_a",
        &[("FLAG_A = 2", true)],
    );

    // C3': module_c.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["module_c.py"]);

    // C4': config_b.py (human-resolved to BATCH=75).
    // diff_based: old=C4's "FLAG_B=1\nBATCH=200\n", new="FLAG_B=1\nBATCH=75\n"
    //   Line 1 "FLAG_B=1": Equal → AI (in note)
    //   Line 2 "BATCH=75" vs "BATCH=200": Replace → human (no note entry)
    // git blame at C4':
    //   Line 1 "FLAG_B = 1": unchanged from parent (main already had FLAG_B=1) → traces to main → human
    //   Line 2 "BATCH = 75": C4' introduced (parent had BATCH=50) → C4' note → no AI entry → human
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["config_b.py"]);

    assert_blame_at_commit(
        &repo,
        &chain[3],
        "config_b.py",
        "c4_blame_config_b",
        &[
            ("FLAG_B = 1", false), // traced to main branch commit (FLAG_B=1 was set by main)
            ("BATCH = 75", false), // C4' introduced, but no AI attribution (Replace in resolution)
        ],
    );

    // C5': module_e.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["module_e.py"]);
}

// ============================================================================
// Category 5: Path-specific correctness tests
// ============================================================================

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
    test_conflict_mixed_ai_and_human_resolve_different_commits,
    test_conflict_working_log_is_sole_attribution_source,
    test_conflict_content_diff_wins_over_working_log,
);
