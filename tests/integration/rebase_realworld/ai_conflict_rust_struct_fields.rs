use super::*;

/// Test 8: models.rs struct fields — feature (C3) AI adds 4 new fields,
/// main human adds 2 different fields.  AI resolution merges all 8 fields.
/// The merged struct body is all .ai().
#[test]
fn test_conflict_ai_resolves_rust_struct_fields() {
    run_test_conflict_ai_resolves_rust_struct_fields(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_rust_struct_fields_standard_human() {
    run_test_conflict_ai_resolves_rust_struct_fields(HumanContextAttribution::Unattributed);
}

fn run_test_conflict_ai_resolves_rust_struct_fields(human_context: HumanContextAttribution) {
    let repo = TestRepo::new();

    // Initial: models.rs with a struct (2 original fields, human)
    repo.commit_untracked_file(
        "src/models.rs",
        "pub struct User {\n    pub id: u64,\n    pub name: String,\n}\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: adds email and created_at fields → will conflict
    repo.commit_untracked_file("src/models.rs",
        "pub struct User {\n    pub id: u64,\n    pub name: String,\n    pub email: String,\n    pub created_at: u64,\n}\n",
        "main: add email and created_at to User",
    );
    repo.commit_untracked_file(
        "src/db.rs",
        "pub struct Db { url: String }\n",
        "main: add Db",
    );
    repo.commit_untracked_file(
        "src/repo.rs",
        "use crate::models::User;\npub struct UserRepo;\n",
        "main: add UserRepo",
    );
    repo.commit_untracked_file(
        "src/service.rs",
        "pub struct UserService;\n",
        "main: add UserService",
    );
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"models\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "main: add Cargo.toml",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates traits.rs (8 AI lines)
    let mut traits = repo.filename("src/traits.rs");
    traits.set_contents(crate::lines![
        "pub trait Entity {".ai(),
        "    fn id(&self) -> u64;".ai(),
        "    fn name(&self) -> &str;".ai(),
        "}".ai(),
        "".ai(),
        "pub trait Persistable: Entity {".ai(),
        "    fn save(&self) -> Result<(), String>;".ai(),
        "    fn delete(&self) -> Result<(), String>;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add Entity and Persistable traits")
        .unwrap();

    // C2: AI creates impls.rs (8 AI lines)
    let mut impls = repo.filename("src/impls.rs");
    impls.set_contents(crate::lines![
        "use crate::models::User;".ai(),
        "use crate::traits::Entity;".ai(),
        "".ai(),
        "impl Entity for User {".ai(),
        "    fn id(&self) -> u64 { self.id }".ai(),
        "    fn name(&self) -> &str { &self.name }".ai(),
        "}".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 impl Entity for User")
        .unwrap();

    // C3: AI adds 4 new fields to User struct — WILL CONFLICT with main's email/created_at
    let mut models = repo.filename("src/models.rs");
    models.set_contents(crate::lines![
        human_context.expected_line("pub struct User {"),
        human_context.expected_line("    pub id: u64,"),
        human_context.expected_line("    pub name: String,"),
        "    pub active: bool,".ai(),
        "    pub role: String,".ai(),
        "    pub score: f64,".ai(),
        "    pub metadata: std::collections::HashMap<String, String>,".ai(),
        human_context.expected_line("}"),
    ]);
    fs::write(
        repo.path().join("src/models.rs"),
        "pub struct User {\n    pub id: u64,\n    pub name: String,\n    pub active: bool,\n    pub role: String,\n    pub score: f64,\n    pub metadata: std::collections::HashMap<String, String>,\n}\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/models.rs"])
        .unwrap();
    repo.stage_all_and_commit("feat: C3 AI adds active/role/score/metadata fields")
        .unwrap();

    // C4: AI creates errors.rs (8 AI lines)
    let mut errors = repo.filename("src/errors.rs");
    errors.set_contents(crate::lines![
        "#[derive(Debug)]".ai(),
        "pub enum ModelError {".ai(),
        "    NotFound(u64),".ai(),
        "    InvalidField(String),".ai(),
        "    DuplicateId(u64),".ai(),
        "}".ai(),
        "".ai(),
        "impl std::fmt::Display for ModelError { fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { write!(f, \"{:?}\", self) } }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add ModelError")
        .unwrap();

    // C5: AI creates utils.rs (8 AI lines)
    let mut utils = repo.filename("src/utils.rs");
    utils.set_contents(crate::lines![
        "pub fn slugify(s: &str) -> String {".ai(),
        "    s.to_lowercase()".ai(),
        "        .chars()".ai(),
        "        .map(|c| if c.is_alphanumeric() { c } else { '-' })".ai(),
        "        .collect::<String>()".ai(),
        "        .trim_matches('-')".ai(),
        "        .to_string()".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add slugify utility")
        .unwrap();

    // Rebase — C3 will conflict on src/models.rs
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/models.rs at C3"
    );

    // AI resolves: merges ALL fields — original 2 + 4 feature + 2 main = 8 fields (all .ai() in struct body)
    let mut conflict_models = repo.filename("src/models.rs");
    conflict_models.set_contents(crate::lines![
        human_context.expected_line("pub struct User {"),
        "    pub id: u64,".ai(),
        "    pub name: String,".ai(),
        "    pub email: String,".ai(),
        "    pub created_at: u64,".ai(),
        "    pub active: bool,".ai(),
        "    pub role: String,".ai(),
        "    pub score: f64,".ai(),
        "    pub metadata: std::collections::HashMap<String, String>,".ai(),
        human_context.expected_line("}"),
    ]);
    fs::write(
        repo.path().join("src/models.rs"),
        "pub struct User {\n    pub id: u64,\n    pub name: String,\n    pub email: String,\n    pub created_at: u64,\n    pub active: bool,\n    pub role: String,\n    pub score: f64,\n    pub metadata: std::collections::HashMap<String, String>,\n}\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/models.rs"])
        .unwrap();
    repo.git(&["add", "src/models.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': traits.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/traits.rs"]);

    // C2': impls.rs only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/impls.rs"]);

    // C3': models.rs only (AI-resolved struct with merged fields)
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/models.rs"]);

    // blame for models.rs: struct keyword is human, equal fields carry AI attribution, new fields are human
    assert_blame_at_commit(
        &repo,
        &chain[2],
        "src/models.rs",
        "c3_blame_models",
        &[
            ("pub struct User {", false),
            ("pub id: u64,", false),
            ("pub name: String,", false),
            ("pub email: String,", false),
            ("pub created_at: u64,", false),
            ("pub active: bool,", true),
            ("pub role: String,", true),
            ("pub score: f64,", true),
            ("pub metadata:", true),
            ("}", false),
        ],
    );

    // C4': errors.rs only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/errors.rs"]);

    // C5': utils.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/utils.rs"]);

    human_context.assert_metadata_humans(&repo, &chain[2], "c3'");
}

crate::reuse_tests_in_worktree!(
    test_conflict_ai_resolves_rust_struct_fields,
    test_conflict_ai_resolves_rust_struct_fields_standard_human,
);
