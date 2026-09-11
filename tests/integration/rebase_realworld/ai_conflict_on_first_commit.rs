use super::*;

/// Test 4: version.py — conflict is on C1 (the VERY FIRST feature commit).
/// Feature changes VERSION to "2.0", main changes it to "1.5".
/// AI resolves to "2.1".  C2–C5 accumulate other files normally.
#[test]
fn test_conflict_ai_resolves_on_first_commit() {
    run_test_conflict_ai_resolves_on_first_commit(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_on_first_commit_standard_human() {
    run_test_conflict_ai_resolves_on_first_commit(HumanContextAttribution::Unattributed);
}

fn run_test_conflict_ai_resolves_on_first_commit(human_context: HumanContextAttribution) {
    let repo = TestRepo::new();

    // Initial: version.py with VERSION = "1.0"
    repo.commit_untracked_file(
        "version.py",
        "VERSION = \"1.0\"\nCODENAME = \"alpha\"\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: changes VERSION to "1.5" — will conflict with feature's C1
    repo.commit_untracked_file(
        "version.py",
        "VERSION = \"1.5\"\nCODENAME = \"beta\"\n",
        "main: bump version to 1.5",
    );
    repo.commit_untracked_file(
        "CHANGELOG.md",
        "## 1.5\n- Performance improvements\n",
        "main: add changelog",
    );
    repo.commit_untracked_file(
        "CONTRIBUTORS.md",
        "# Contributors\n- Alice\n- Bob\n",
        "main: add contributors",
    );
    repo.commit_untracked_file(
        "LICENSE",
        "MIT License\nCopyright 2024\n",
        "main: add license",
    );
    repo.commit_untracked_file(
        "docs/index.md",
        "# Docs\nWelcome to the docs.\n",
        "main: add docs",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI changes VERSION to "2.0" — WILL CONFLICT
    let mut version = repo.filename("version.py");
    version.set_contents(crate::lines![
        "VERSION = \"2.0\"".ai(),
        human_context.expected_line("CODENAME = \"alpha\""),
    ]);
    repo.stage_all_and_commit("feat: C1 bump version to 2.0")
        .unwrap();

    // C2: AI creates changelog.py (8 AI lines)
    let mut changelog = repo.filename("changelog.py");
    changelog.set_contents(crate::lines![
        "import datetime".ai(),
        "".ai(),
        "class ChangelogEntry:".ai(),
        "    def __init__(self, version: str, date: datetime.date, changes: list):".ai(),
        "        self.version = version".ai(),
        "        self.date = date".ai(),
        "        self.changes = changes".ai(),
        "    def render(self) -> str: return f'{self.version} ({self.date}): {len(self.changes)} changes'".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add changelog model")
        .unwrap();

    // C3: AI creates release_notes.py (8 AI lines)
    let mut release_notes = repo.filename("release_notes.py");
    release_notes.set_contents(crate::lines![
        "from typing import List".ai(),
        "".ai(),
        "def format_release_notes(entries: List[dict]) -> str:".ai(),
        "    lines = []".ai(),
        "    for e in entries:".ai(),
        "        lines.append(f\"## {e['version']}\")".ai(),
        "        for change in e.get('changes', []):".ai(),
        "            lines.append(f'- {change}')".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add release notes formatter")
        .unwrap();

    // C4: AI creates deprecations.py (8 AI lines)
    let mut deprecations = repo.filename("deprecations.py");
    deprecations.set_contents(crate::lines![
        "import warnings".ai(),
        "import functools".ai(),
        "".ai(),
        "def deprecated(reason: str):".ai(),
        "    def decorator(func):".ai(),
        "        @functools.wraps(func)".ai(),
        "        def wrapper(*args, **kwargs):".ai(),
        "            warnings.warn(f'{func.__name__} is deprecated: {reason}', DeprecationWarning, stacklevel=2)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add deprecation decorator")
        .unwrap();

    // C5: AI creates migration_guide.py (8 AI lines)
    let mut migration_guide = repo.filename("migration_guide.py");
    migration_guide.set_contents(crate::lines![
        "MIGRATION_STEPS = [".ai(),
        "    'Update config files to new schema',".ai(),
        "    'Run database migration scripts',".ai(),
        "    'Update API call signatures',".ai(),
        "    'Test all integrations',".ai(),
        "    'Deploy to staging first',".ai(),
        "    'Monitor error rates after deployment',".ai(),
        "]".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add migration guide")
        .unwrap();

    // Rebase — C1 will conflict immediately on version.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on version.py at C1"
    );

    // AI resolves: VERSION = "2.1" as .ai(), CODENAME as the selected human attribution
    let mut conflict_version = repo.filename("version.py");
    conflict_version.set_contents(crate::lines![
        "VERSION = \"2.1\"".ai(),
        human_context.expected_line("CODENAME = \"beta\""),
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': version.py only with AI-resolved VERSION line (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["version.py"]);

    // blame at chain[0] for version.py: VERSION line is AI, CODENAME is human
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "version.py",
        "c1_blame_version",
        &[("VERSION = \"2.1\"", true), ("CODENAME = \"beta\"", false)],
    );

    // C2': changelog.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["changelog.py"]);

    // C3': release_notes.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["release_notes.py"]);

    // C4': deprecations.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["deprecations.py"]);

    // C5': migration_guide.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["migration_guide.py"]);

    human_context.assert_metadata_humans(&repo, &chain[0], "c1'");
}

crate::reuse_tests_in_worktree!(
    test_conflict_ai_resolves_on_first_commit,
    test_conflict_ai_resolves_on_first_commit_standard_human,
);
