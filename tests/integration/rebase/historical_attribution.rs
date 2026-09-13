use super::{AuthorshipLog, ExpectedLineExt, TestRepo, leading_dropped_commits_before_first_match};

/// Regression test: AI attribution from earlier commits (not HEAD) must survive rebase.
///
/// Each commit's note only covers lines changed in THAT commit. HEAD doesn't
/// touch all AI-attributed files. The reconstruction must process ALL commits'
/// notes to build the complete attribution state, not just HEAD's.
#[test]
fn test_rebase_preserves_attribution_from_non_head_commits() {
    let repo = TestRepo::new();

    // Initial commit
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    // Feature branch: commit 1 — AI attribution on file_a only
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut file_a = repo.filename("file_a.txt");
    file_a.set_contents(crate::lines![
        "// AI generated module A".ai(),
        "fn module_a() {}".ai(),
        "// end module A".ai()
    ]);
    repo.stage_all_and_commit("feat: add module A (AI)")
        .unwrap();

    // Feature branch: commit 2 — AI attribution on file_b only (file_a not touched)
    let mut file_b = repo.filename("file_b.txt");
    file_b.set_contents(crate::lines![
        "// AI generated module B".ai(),
        "fn module_b() {}".ai()
    ]);
    repo.stage_all_and_commit("feat: add module B (AI)")
        .unwrap();

    // Feature branch: commit 3 (HEAD) — AI attribution on file_c only
    // file_a and file_b are NOT touched in this commit
    let mut file_c = repo.filename("file_c.txt");
    file_c.set_contents(crate::lines![
        "// AI generated module C".ai(),
        "fn module_c() {}".ai()
    ]);
    repo.stage_all_and_commit("feat: add module C (AI)")
        .unwrap();

    // Advance main branch to force actual rebase (not fast-forward)
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut main_change = repo.filename("main_update.txt");
    main_change.set_contents(crate::lines!["main branch work"]);
    repo.stage_all_and_commit("main: infrastructure update")
        .unwrap();

    // Rebase feature onto main
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // CRITICAL: file_a attribution (from commit 1, NOT HEAD) must survive
    file_a.assert_lines_and_blame(crate::lines![
        "// AI generated module A".ai(),
        "fn module_a() {}".ai(),
        "// end module A".ai()
    ]);

    // file_b attribution (from commit 2, NOT HEAD) must survive
    file_b.assert_lines_and_blame(crate::lines![
        "// AI generated module B".ai(),
        "fn module_b() {}".ai()
    ]);

    // file_c attribution (from HEAD commit) must survive
    file_c.assert_lines_and_blame(crate::lines![
        "// AI generated module C".ai(),
        "fn module_c() {}".ai()
    ]);
}

/// Regression test: multi-commit attribution on SAME file from different commits.
///
/// Commit 1 adds AI lines 1-3, commit 3 adds AI lines 4-6, but commit 2
/// (between them) touches a different file entirely. The reconstruction must
/// combine notes from both commits to get the full attribution for the file.
#[test]
fn test_rebase_preserves_multi_commit_attribution_same_file() {
    let repo = TestRepo::new();

    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Commit 1: AI attribution on app.txt lines 1-3
    let mut app = repo.filename("app.txt");
    app.set_contents(crate::lines![
        "// AI header".ai(),
        "fn init() {}".ai(),
        "// end init".ai()
    ]);
    repo.stage_all_and_commit("feat: AI init code").unwrap();

    // Commit 2: touch a DIFFERENT file (app.txt unchanged)
    let mut config = repo.filename("config.txt");
    config.set_contents(crate::lines!["// AI config".ai(), "setting = true".ai()]);
    repo.stage_all_and_commit("feat: AI config").unwrap();

    // Commit 3 (HEAD): add MORE AI lines to app.txt
    app.set_contents(crate::lines![
        "// AI header".ai(),
        "fn init() {}".ai(),
        "// end init".ai(),
        "// AI footer added later".ai(),
        "fn cleanup() {}".ai()
    ]);
    repo.stage_all_and_commit("feat: AI cleanup code").unwrap();

    // Advance main
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut infra = repo.filename("infra.txt");
    infra.set_contents(crate::lines!["infra work"]);
    repo.stage_all_and_commit("main: infra").unwrap();

    // Rebase
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // app.txt should have ALL AI lines (from commits 1 AND 3)
    app.assert_lines_and_blame(crate::lines![
        "// AI header".ai(),
        "fn init() {}".ai(),
        "// end init".ai(),
        "// AI footer added later".ai(),
        "fn cleanup() {}".ai()
    ]);

    // config.txt (from commit 2, NOT HEAD) must survive
    config.assert_lines_and_blame(crate::lines!["// AI config".ai(), "setting = true".ai()]);
}

/// Regression test: attribution survives when main branch modifies AI-attributed
/// files, forcing the slow path (blob OID mismatch between original and rebased).
/// This tests that attribution from non-HEAD commits survives even through the
/// full attribution rewrite path.
#[test]
fn test_rebase_non_head_attribution_survives_slow_path() {
    let repo = TestRepo::new();

    let mut base = repo.filename("shared.txt");
    base.set_contents(crate::lines![
        "// top section",
        "line_a",
        "line_b",
        "line_c",
        "",
        "",
        "",
        "",
        "// bottom section",
        "line_x",
        "line_y",
        "line_z"
    ]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // Commit 1: AI attribution on module.txt
    let mut module = repo.filename("module.txt");
    module.set_contents(crate::lines![
        "// AI module".ai(),
        "pub fn process() {}".ai(),
        "// end".ai()
    ]);
    repo.stage_all_and_commit("feat: AI module").unwrap();

    // Commit 2 (HEAD): append to bottom of shared.txt
    // module.txt is NOT touched here
    let mut shared = repo.filename("shared.txt");
    shared.set_contents(crate::lines![
        "// top section",
        "line_a",
        "line_b",
        "line_c",
        "",
        "",
        "",
        "",
        "// bottom section",
        "line_x",
        "line_y",
        "line_z",
        "// feature addition".ai()
    ]);
    repo.stage_all_and_commit("feat: extend shared").unwrap();

    // Advance main — add a new file so the rebase replays commits on a new base.
    // shared.txt is NOT modified on main, so no merge conflict occurs.
    repo.git(&["checkout", &default_branch]).unwrap();
    let mut infra = repo.filename("infra.txt");
    infra.set_contents(crate::lines!["// infrastructure", "setup_logging();"]);
    repo.stage_all_and_commit("main: add infra file").unwrap();

    // Rebase
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();

    // module.txt attribution (from commit 1, NOT HEAD) must survive
    // even though the rebase took the slow path due to shared.txt changes
    module.assert_lines_and_blame(crate::lines![
        "// AI module".ai(),
        "pub fn process() {}".ai(),
        "// end".ai()
    ]);
}

#[test]
fn test_eng_279_reset_keep_discards_divergent_trunk_mappings() {
    use std::fs;

    const TRUNK_COMMIT_COUNT: usize = 65;

    let repo = TestRepo::new_with_daemon_env(&[("GIT_AI_DEBUG", "1")]);
    let mut base_file = repo.filename("base.txt");
    base_file.set_contents(crate::lines!["base".human()]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    base_file.assert_committed_lines(crate::lines!["base".human()]);

    let base_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let feature_path = repo.path().join("feature.txt");
    fs::write(&feature_path, "ai feature line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "feature.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("feature commit").unwrap();
    let original_feature_tip = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let mut feature_file = repo.filename("feature.txt");
    feature_file.assert_committed_lines(crate::lines!["ai feature line".ai()]);

    repo.git(&["checkout", &default_branch]).unwrap();
    let trunk_path = repo.path().join("trunk.txt");
    let mut trunk_file = repo.filename("trunk.txt");
    for commit_number in 1..=TRUNK_COMMIT_COUNT {
        let contents = (1..=commit_number)
            .map(|line| format!("ai trunk line {line}\n"))
            .collect::<String>();
        fs::write(&trunk_path, contents).unwrap();
        repo.git_ai(&["checkpoint", "mock_ai", "trunk.txt"])
            .unwrap();
        repo.git(&["add", "-A"]).unwrap();
        repo.commit(&format!("trunk commit {commit_number}"))
            .unwrap();
        trunk_file.assert_committed_lines(
            (1..=commit_number)
                .map(|line| format!("ai trunk line {line}").ai())
                .collect(),
        );
    }

    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &default_branch]).unwrap();
    let rebased_feature_tip = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_ne!(original_feature_tip, rebased_feature_tip);
    feature_file.assert_committed_lines(crate::lines!["ai feature line".ai()]);

    let old_range = format!("{base_sha}..{rebased_feature_tip}");
    let new_range = format!("{base_sha}..{original_feature_tip}");
    let range_diff = repo
        .git(&[
            "range-diff",
            "--no-abbrev",
            "-s",
            "--creation-factor=100",
            &old_range,
            &new_range,
        ])
        .unwrap();
    assert!(
        leading_dropped_commits_before_first_match(&range_diff) > 64,
        "test setup must exceed the leading-drop bound:\n{range_diff}"
    );

    #[cfg(not(windows))]
    let (daemon_log_path, daemon_log_len_before_reset) = {
        let path = repo
            .test_home_path()
            .join(".git-ai")
            .join("internal")
            .join("daemon")
            .join("daemon.test.stderr.log");
        let len = fs::metadata(&path).unwrap().len() as usize;
        (path, len)
    };

    repo.git(&["reset", "--keep", &original_feature_tip])
        .unwrap();
    repo.sync_daemon();
    assert_eq!(
        repo.git(&["rev-parse", "HEAD"]).unwrap().trim(),
        original_feature_tip
    );
    assert!(!trunk_path.exists());
    feature_file.assert_committed_lines(crate::lines!["ai feature line".ai()]);

    let note = repo
        .read_authorship_note(&original_feature_tip)
        .expect("original feature commit should retain its authorship note");
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse authorship note");
    assert!(
        log.attestations
            .iter()
            .any(|attestation| attestation.file_path == "feature.txt")
    );
    assert!(
        log.attestations
            .iter()
            .all(|attestation| attestation.file_path != "trunk.txt"),
        "divergent trunk mappings must not contaminate the feature note"
    );

    // Windows test daemons do not reliably emit tracing diagnostics to their
    // stderr file before shutdown. The cross-platform range-diff unit tests
    // prove the mapping bound; keep this end-to-end diagnostic assertion on
    // platforms where the daemon log is synchronously observable.
    #[cfg(not(windows))]
    {
        let daemon_log = fs::read_to_string(&daemon_log_path).unwrap();
        let reset_log = &daemon_log[daemon_log_len_before_reset..];
        assert!(
            reset_log.contains("shift_authorship_notes: 1 mappings"),
            "restack undo must shift only the real feature mapping:\n{reset_log}"
        );
    }
}
