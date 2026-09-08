use super::{
    ExpectedLineExt, HumanContextAttribution, TestRepo, assert_blame_at_commit,
    assert_note_base_commit_matches, assert_note_files_exact, fs, get_commit_chain,
};

/// Test 5: schema.rs max_connections — conflict is on C5 (LAST feature commit).
/// C1–C4 accumulate model_*.rs files cleanly.  C5 modifies schema.rs
/// max_connections constant; main also modifies same constant.  AI resolves.
#[test]
fn test_conflict_ai_resolves_on_last_commit() {
    run_test_conflict_ai_resolves_on_last_commit(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_on_last_commit_standard_human() {
    run_test_conflict_ai_resolves_on_last_commit(HumanContextAttribution::Unattributed);
}

fn run_test_conflict_ai_resolves_on_last_commit(human_context: HumanContextAttribution) {
    let repo = TestRepo::new();

    // Initial: schema.rs with a constant (human)
    repo.commit_untracked_file(
        "src/schema.rs",
        "pub const MAX_CONNECTIONS: u32 = 10;\npub const SCHEMA_VERSION: u32 = 1;\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: changes max_connections → will conflict with feature's C5
    repo.commit_untracked_file(
        "src/schema.rs",
        "pub const MAX_CONNECTIONS: u32 = 50;\npub const SCHEMA_VERSION: u32 = 1;\n",
        "main: increase max_connections to 50",
    );
    repo.commit_untracked_file(
        "src/migration.rs",
        "pub fn run_migrations() {}\n",
        "main: add migration runner",
    );
    repo.commit_untracked_file(
        "src/connection.rs",
        "pub struct Connection { id: u32 }\n",
        "main: add Connection type",
    );
    repo.commit_untracked_file(
        "src/pool.rs",
        "pub struct Pool { size: u32 }\n",
        "main: add Pool struct",
    );
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"schema\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "main: add Cargo.toml",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates model_a.rs (10 AI lines)
    let mut model_a = repo.filename("src/model_a.rs");
    model_a.set_contents(crate::lines![
        "#[derive(Debug, Clone)]".ai(),
        "pub struct ModelA {".ai(),
        "    pub id: u64,".ai(),
        "    pub name: String,".ai(),
        "    pub active: bool,".ai(),
        "}".ai(),
        "".ai(),
        "impl ModelA {".ai(),
        "    pub fn new(id: u64, name: impl Into<String>) -> Self {".ai(),
        "        Self { id, name: name.into(), active: true }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add ModelA").unwrap();

    // C2: AI creates model_b.rs (10 AI lines)
    let mut model_b = repo.filename("src/model_b.rs");
    model_b.set_contents(crate::lines![
        "#[derive(Debug, Clone)]".ai(),
        "pub struct ModelB {".ai(),
        "    pub id: u64,".ai(),
        "    pub value: f64,".ai(),
        "    pub tags: Vec<String>,".ai(),
        "}".ai(),
        "".ai(),
        "impl ModelB {".ai(),
        "    pub fn new(id: u64, value: f64) -> Self {".ai(),
        "        Self { id, value, tags: Vec::new() }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add ModelB").unwrap();

    // C3: AI creates model_c.rs (10 AI lines)
    let mut model_c = repo.filename("src/model_c.rs");
    model_c.set_contents(crate::lines![
        "#[derive(Debug, Clone, PartialEq)]".ai(),
        "pub enum Status {".ai(),
        "    Active,".ai(),
        "    Inactive,".ai(),
        "    Pending,".ai(),
        "}".ai(),
        "".ai(),
        "impl Default for Status {".ai(),
        "    fn default() -> Self { Status::Pending }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add Status enum")
        .unwrap();

    // C4: AI creates model_d.rs (10 AI lines)
    let mut model_d = repo.filename("src/model_d.rs");
    model_d.set_contents(crate::lines![
        "use std::collections::HashMap;".ai(),
        "".ai(),
        "#[derive(Debug, Default)]".ai(),
        "pub struct Registry {".ai(),
        "    entries: HashMap<u64, String>,".ai(),
        "}".ai(),
        "".ai(),
        "impl Registry {".ai(),
        "    pub fn register(&mut self, id: u64, name: impl Into<String>) {".ai(),
        "        self.entries.insert(id, name.into());".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add Registry").unwrap();

    // C5: AI changes max_connections to 100 — WILL CONFLICT
    let mut schema = repo.filename("src/schema.rs");
    schema.set_contents(crate::lines![
        "pub const MAX_CONNECTIONS: u32 = 100;".ai(),
        human_context.expected_line("pub const SCHEMA_VERSION: u32 = 1;"),
    ]);
    fs::write(
        repo.path().join("src/schema.rs"),
        "pub const MAX_CONNECTIONS: u32 = 100;\npub const SCHEMA_VERSION: u32 = 1;\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/schema.rs"])
        .unwrap();
    repo.stage_all_and_commit("feat: C5 AI tunes MAX_CONNECTIONS to 100")
        .unwrap();

    // Rebase — C5 will conflict on src/schema.rs
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/schema.rs at C5"
    );

    // AI resolves: picks 75 as a compromise, as .ai()
    let mut conflict_schema = repo.filename("src/schema.rs");
    conflict_schema.set_contents(crate::lines![
        "pub const MAX_CONNECTIONS: u32 = 75;".ai(),
        human_context.expected_line("pub const SCHEMA_VERSION: u32 = 1;"),
    ]);
    fs::write(
        repo.path().join("src/schema.rs"),
        "pub const MAX_CONNECTIONS: u32 = 75;\npub const SCHEMA_VERSION: u32 = 1;\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/schema.rs"])
        .unwrap();
    repo.git(&["add", "src/schema.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': model_a.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/model_a.rs"]);

    // C2': model_b.rs only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/model_b.rs"]);

    // C3': model_c.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/model_c.rs"]);

    // C4': model_d.rs only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/model_d.rs"]);

    // C5': schema.rs only (AI-resolved MAX_CONNECTIONS)
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/schema.rs"]);

    // blame at chain[4] for schema.rs: MAX_CONNECTIONS line is AI, SCHEMA_VERSION is human
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "src/schema.rs",
        "c5_blame_schema",
        &[
            ("MAX_CONNECTIONS: u32 = 75", true),
            ("SCHEMA_VERSION: u32 = 1", false),
        ],
    );

    human_context.assert_metadata_humans(&repo, &chain[4], "c5'");
}

crate::reuse_tests_in_worktree!(
    test_conflict_ai_resolves_on_last_commit,
    test_conflict_ai_resolves_on_last_commit_standard_human,
);
