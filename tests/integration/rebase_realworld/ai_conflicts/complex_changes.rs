use super::*;

/// Test 6: config.py AND settings.py both conflict in C3.
/// C3 AI changes a line in both files; main also changes same lines.
/// AI resolves both conflicts.  Note for C3' has both files.
#[test]
fn test_conflict_ai_resolves_multiple_files_in_same_commit() {
    run_test_conflict_ai_resolves_multiple_files_in_same_commit(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_multiple_files_in_same_commit_standard_human() {
    run_test_conflict_ai_resolves_multiple_files_in_same_commit(
        HumanContextAttribution::Unattributed,
    );
}

fn run_test_conflict_ai_resolves_multiple_files_in_same_commit(
    human_context: HumanContextAttribution,
) {
    let repo = TestRepo::new();

    // Initial: BOTH files exist at the shared base so C3's edits will conflict with main
    repo.commit_untracked_file(
        "config.py",
        "DEBUG = False\nSECRET_KEY = 'changeme'\n",
        "Initial: config",
    );
    repo.commit_untracked_file(
        "settings.py",
        "DATABASE_URL = 'sqlite:///dev.db'\nCACHE_BACKEND = 'locmem'\n",
        "Initial: settings",
    );
    let main_branch = repo.current_branch();

    // Main: changes the same lines in both files → will conflict with feature's C3
    repo.commit_untracked_file(
        "config.py",
        "DEBUG = True\nSECRET_KEY = 'changeme'\n",
        "main: enable DEBUG",
    );
    repo.commit_untracked_file(
        "settings.py",
        "DATABASE_URL = 'postgres://localhost/main_db'\nCACHE_BACKEND = 'redis'\n",
        "main: update settings",
    );
    repo.commit_untracked_file(
        "wsgi.py",
        "from app import create_app\napplication = create_app()\n",
        "main: add wsgi",
    );
    repo.commit_untracked_file(
        "asgi.py",
        "from app import create_app\napplication = create_app()\n",
        "main: add asgi",
    );
    repo.commit_untracked_file(
        "manage.py",
        "#!/usr/bin/env python\nimport sys\nif __name__ == '__main__': pass\n",
        "main: add manage.py",
    );

    // Feature branch from the shared base (HEAD~5 = after both initial commits)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates auth.py (8 AI lines)
    let mut auth = repo.filename("auth.py");
    auth.set_contents(crate::lines![
        "from typing import Optional".ai(),
        "".ai(),
        "def authenticate(token: str) -> Optional[str]:".ai(),
        "    if not token: return None".ai(),
        "    parts = token.split('.')".ai(),
        "    if len(parts) != 3: return None".ai(),
        "    return parts[1]".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add auth").unwrap();

    // C2: AI creates middleware.py (8 AI lines)
    let mut middleware = repo.filename("middleware.py");
    middleware.set_contents(crate::lines![
        "class CorsMiddleware:".ai(),
        "    def __init__(self, app):".ai(),
        "        self.app = app".ai(),
        "    def __call__(self, environ, start_response):".ai(),
        "        def custom_start(status, headers):".ai(),
        "            headers.append(('Access-Control-Allow-Origin', '*'))".ai(),
        "            return start_response(status, headers)".ai(),
        "        return self.app(environ, custom_start)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add CORS middleware")
        .unwrap();

    // C3: AI changes config.py AND settings.py — BOTH WILL CONFLICT
    let mut config = repo.filename("config.py");
    config.set_contents(crate::lines![
        human_context.expected_line("DEBUG = False"),
        "SECRET_KEY = 'ai-generated-secret-key-v2'".ai(),
    ]);
    let mut settings = repo.filename("settings.py");
    settings.set_contents(crate::lines![
        "DATABASE_URL = 'postgres://localhost/feature_db'".ai(),
        human_context.expected_line("CACHE_BACKEND = 'locmem'"),
    ]);
    repo.stage_all_and_commit("feat: C3 AI tunes config and settings")
        .unwrap();

    // C4: AI creates permissions.py (8 AI lines)
    let mut permissions = repo.filename("permissions.py");
    permissions.set_contents(crate::lines![
        "class Permission:".ai(),
        "    READ = 'read'".ai(),
        "    WRITE = 'write'".ai(),
        "    ADMIN = 'admin'".ai(),
        "".ai(),
        "def has_permission(user_perms: list, required: str) -> bool:".ai(),
        "    return required in user_perms".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add permissions")
        .unwrap();

    // C5: AI creates serializers.py (8 AI lines)
    let mut serializers = repo.filename("serializers.py");
    serializers.set_contents(crate::lines![
        "import json".ai(),
        "".ai(),
        "class JsonSerializer:".ai(),
        "    @staticmethod".ai(),
        "    def dumps(obj) -> str: return json.dumps(obj)".ai(),
        "    @staticmethod".ai(),
        "    def loads(s: str): return json.loads(s)".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add JSON serializer")
        .unwrap();

    // Rebase — C3 will conflict on config.py (and possibly settings.py)
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(rebase_result.is_err(), "rebase should conflict at C3");

    // AI resolves config.py
    let mut conflict_config = repo.filename("config.py");
    conflict_config.set_contents(crate::lines![
        human_context.expected_line("DEBUG = True"),
        "SECRET_KEY = 'ai-generated-secret-key-v2'".ai(),
    ]);
    // AI resolves settings.py
    let mut conflict_settings = repo.filename("settings.py");
    conflict_settings.set_contents(crate::lines![
        "DATABASE_URL = 'postgres://localhost/feature_db'".ai(),
        human_context.expected_line("CACHE_BACKEND = 'redis'"),
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': auth.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["auth.py"]);

    // C2': middleware.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["middleware.py"]);

    // C3': config.py + settings.py (AI-resolved, both in same commit)
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["config.py", "settings.py"]);

    // blame for config.py: DEBUG is human (unchanged), SECRET_KEY is AI
    assert_blame_at_commit(
        &repo,
        &chain[2],
        "config.py",
        "c3_blame_config",
        &[
            ("DEBUG = True", false),
            ("SECRET_KEY = 'ai-generated-secret-key-v2'", true),
        ],
    );

    // blame for settings.py: DATABASE_URL is AI, CACHE_BACKEND is human
    assert_blame_at_commit(
        &repo,
        &chain[2],
        "settings.py",
        "c3_blame_settings",
        &[
            ("DATABASE_URL = 'postgres://localhost/feature_db'", true),
            ("CACHE_BACKEND = 'redis'", false),
        ],
    );

    // C4': permissions.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["permissions.py"]);

    // C5': serializers.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["serializers.py"]);

    human_context.assert_metadata_humans(&repo, &chain[2], "c3'");
}

/// Test 7: dispatcher.py — conflict on C2.  C3 and C4 also modify dispatcher.py
/// (no further conflicts).  AI resolves C2 with 12-line process() implementation.
/// Subsequent commits append more methods to dispatcher.py.
#[test]
fn test_conflict_ai_resolves_then_more_ai_builds_on_result() {
    run_test_conflict_ai_resolves_then_more_ai_builds_on_result(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_then_more_ai_builds_on_result_standard_human() {
    run_test_conflict_ai_resolves_then_more_ai_builds_on_result(
        HumanContextAttribution::Unattributed,
    );
}

fn run_test_conflict_ai_resolves_then_more_ai_builds_on_result(
    human_context: HumanContextAttribution,
) {
    let repo = TestRepo::new();

    // Initial: dispatcher.py stub (human)
    repo.commit_untracked_file(
        "dispatcher.py",
        "class Dispatcher:\n    pass\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: human implements process() differently → will conflict with feature's C2
    repo.commit_untracked_file(
        "dispatcher.py",
        "class Dispatcher:\n    def process(self, msg): return msg.strip()\n",
        "main: implement process() simply",
    );
    repo.commit_untracked_file(
        "config.py",
        "WORKERS = 4\nQUEUE_SIZE = 100\n",
        "main: add config",
    );
    repo.commit_untracked_file(
        "queue.py",
        "import queue\nQ = queue.Queue()\n",
        "main: add queue",
    );
    repo.commit_untracked_file(
        "worker.py",
        "class Worker:\n    def __init__(self, q): self.q = q\n",
        "main: add worker",
    );
    repo.commit_untracked_file(
        "monitor.py",
        "class Monitor:\n    def check(self): return 'ok'\n",
        "main: add monitor",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates base_handler.py (8 AI lines)
    let mut base_handler = repo.filename("base_handler.py");
    base_handler.set_contents(crate::lines![
        "class BaseHandler:".ai(),
        "    def __init__(self):".ai(),
        "        self.middlewares = []".ai(),
        "    def use(self, middleware):".ai(),
        "        self.middlewares.append(middleware)".ai(),
        "        return self".ai(),
        "    def handle(self, msg): raise NotImplementedError".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add BaseHandler")
        .unwrap();

    // C2: AI adds process() to dispatcher.py — WILL CONFLICT
    let mut dispatcher_c2 = repo.filename("dispatcher.py");
    dispatcher_c2.set_contents(crate::lines![
        human_context.expected_line("class Dispatcher:"),
        "    def process(self, msg):".ai(),
        "        msg = msg.strip()".ai(),
        "        if not msg: raise ValueError('empty')".ai(),
        "        tokens = msg.split()".ai(),
        "        return {'cmd': tokens[0], 'args': tokens[1:]}".ai(),
        human_context.expected_line("    pass"),
    ]);
    repo.stage_all_and_commit("feat: C2 AI adds process() to Dispatcher")
        .unwrap();

    // C3: AI creates router.py (does NOT touch dispatcher.py — no conflict)
    let mut router = repo.filename("router.py");
    router.set_contents(crate::lines![
        "from dispatcher import Dispatcher".ai(),
        "".ai(),
        "class Router:".ai(),
        "    def __init__(self):".ai(),
        "        self.dispatcher = Dispatcher()".ai(),
        "    def register(self, cmd, fn): self.dispatcher.route(cmd, fn)".ai(),
        "    def run(self, msg): return self.dispatcher.dispatch(msg)".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 AI adds Router")
        .unwrap();

    // C4: AI creates middleware.py (new file, no conflict)
    let mut mw = repo.filename("middleware.py");
    mw.set_contents(crate::lines![
        "class Middleware:".ai(),
        "    def __init__(self): self.chain = []".ai(),
        "    def use(self, fn): self.chain.append(fn); return self".ai(),
        "    def run(self, msg):".ai(),
        "        for fn in self.chain: msg = fn(msg)".ai(),
        "        return msg".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 AI adds Middleware")
        .unwrap();

    // C5: AI creates event_bus.py (new file, no conflict)
    let mut bus = repo.filename("event_bus.py");
    bus.set_contents(crate::lines![
        "class EventBus:".ai(),
        "    def __init__(self): self.handlers = {}".ai(),
        "    def on(self, event, fn): self.handlers.setdefault(event, []).append(fn)".ai(),
        "    def emit(self, event, *args):".ai(),
        "        for fn in self.handlers.get(event, []): fn(*args)".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 AI adds EventBus")
        .unwrap();

    // Rebase — C2 will conflict on dispatcher.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on dispatcher.py at C2"
    );

    // AI resolves C2: 12-line process() implementation (all .ai() except class line)
    let mut conflict_dispatcher = repo.filename("dispatcher.py");
    conflict_dispatcher.set_contents(crate::lines![
        human_context.expected_line("class Dispatcher:"),
        "    def process(self, msg):".ai(),
        "        # AI merge: validates and parses, as in feature branch".ai(),
        "        msg = msg.strip()".ai(),
        "        if not msg: raise ValueError('empty message')".ai(),
        "        tokens = msg.split()".ai(),
        "        cmd = tokens[0].lower()".ai(),
        "        args = tokens[1:]".ai(),
        "        return {'cmd': cmd, 'args': args, 'raw': msg}".ai(),
        "    def _noop(self, args): return None".ai(),
        "    def __repr__(self): return f'Dispatcher()'".ai(),
        human_context.expected_line("    pass"),
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': base_handler.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["base_handler.py"]);

    // C2': dispatcher.py only (AI-resolved: ~10 AI lines)
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["dispatcher.py"]);

    // C3': router.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["router.py"]);

    // C4': middleware.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["middleware.py"]);

    // C5': event_bus.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["event_bus.py"]);

    human_context.assert_metadata_humans(&repo, &chain[1], "c2'");
}

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

/// Test 9: service.py process_payment — feature (C4) AI implements a 20-line
/// function body; main also implements the same function (12 lines).
/// AI resolution produces a 25-line merged implementation (all .ai()).
/// Non-conflict commits: C1 models.py, C2 validators.py, C3 exceptions.py, C5 utils.py.
#[test]
fn test_conflict_ai_resolves_complex_function_with_error_handling() {
    run_test_conflict_ai_resolves_complex_function_with_error_handling(
        HumanContextAttribution::Known,
    );
}

#[test]
fn test_conflict_ai_resolves_complex_function_with_error_handling_standard_human() {
    run_test_conflict_ai_resolves_complex_function_with_error_handling(
        HumanContextAttribution::Unattributed,
    );
}

fn run_test_conflict_ai_resolves_complex_function_with_error_handling(
    human_context: HumanContextAttribution,
) {
    let repo = TestRepo::new();

    // Initial: service.py with a function stub (human)
    repo.commit_untracked_file(
        "service.py",
        "def process_payment(amount, card):\n    pass\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: human implements process_payment differently → will conflict
    repo.commit_untracked_file("service.py",
        "def process_payment(amount, card):\n    if amount <= 0:\n        raise ValueError('amount must be positive')\n    return {'status': 'ok', 'amount': amount}\n",
        "main: implement process_payment",
    );
    repo.commit_untracked_file("tests/test_service.py",
        "from service import process_payment\ndef test_basic(): assert process_payment(10, '4111')['status'] == 'ok'\n",
        "main: add service tests",
    );
    repo.commit_untracked_file(
        "requirements.txt",
        "stripe==5.0.0\nrequests==2.31.0\n",
        "main: add requirements",
    );
    repo.commit_untracked_file(
        ".env.example",
        "STRIPE_KEY=sk_test_xxx\nDATABASE_URL=sqlite:///dev.db\n",
        "main: add .env.example",
    );
    repo.commit_untracked_file(
        "Makefile",
        "test:\n\tpython -m pytest\nlint:\n\tflake8 .\n.PHONY: test lint\n",
        "main: add Makefile",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates models.py (8 AI lines)
    let mut models = repo.filename("models.py");
    models.set_contents(crate::lines![
        "from dataclasses import dataclass, field".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class PaymentResult:".ai(),
        "    status: str".ai(),
        "    transaction_id: str".ai(),
        "    amount: float".ai(),
        "    error: str = ''".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add PaymentResult model")
        .unwrap();

    // C2: AI creates validators.py (8 AI lines)
    let mut validators = repo.filename("validators.py");
    validators.set_contents(crate::lines![
        "import re".ai(),
        "".ai(),
        "def validate_card(card: str) -> bool:".ai(),
        "    return bool(re.match(r'^[0-9]{13,19}$', card.replace(' ', '')))".ai(),
        "".ai(),
        "def validate_amount(amount: float) -> bool:".ai(),
        "    return isinstance(amount, (int, float)) and 0 < amount <= 1_000_000".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add payment validators")
        .unwrap();

    // C3: AI creates exceptions.py (8 AI lines)
    let mut exceptions = repo.filename("exceptions.py");
    exceptions.set_contents(crate::lines![
        "class PaymentError(Exception):".ai(),
        "    def __init__(self, msg: str, code: int = 400):".ai(),
        "        super().__init__(msg)".ai(),
        "        self.code = code".ai(),
        "".ai(),
        "class CardDeclinedError(PaymentError):".ai(),
        "    def __init__(self): super().__init__('Card declined', 402)".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add payment exceptions")
        .unwrap();

    // C4: AI implements process_payment with 20 lines — WILL CONFLICT
    let mut service = repo.filename("service.py");
    service.set_contents(crate::lines![
        human_context.expected_line("def process_payment(amount, card):"),
        "    from validators import validate_amount, validate_card".ai(),
        "    from exceptions import PaymentError, CardDeclinedError".ai(),
        "    import logging".ai(),
        "    logger = logging.getLogger(__name__)".ai(),
        "    logger.info(f'Processing payment: amount={amount}')".ai(),
        "    if not validate_amount(amount):".ai(),
        "        raise PaymentError(f'Invalid amount: {amount}')".ai(),
        "    if not validate_card(card):".ai(),
        "        raise PaymentError(f'Invalid card number')".ai(),
        "    if str(card).startswith('0000'):".ai(),
        "        raise CardDeclinedError()".ai(),
        "    transaction_id = f'txn_{hash(card + str(amount)) % 10**9}'".ai(),
        "    logger.info(f'Payment successful: {transaction_id}')".ai(),
        "    return {'status': 'ok', 'transaction_id': transaction_id, 'amount': amount}".ai(),
        "    # end process_payment".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 AI implements process_payment")
        .unwrap();

    // C5: AI creates utils.py (8 AI lines)
    let mut utils = repo.filename("utils.py");
    utils.set_contents(crate::lines![
        "def mask_card(card: str) -> str:".ai(),
        "    digits = card.replace(' ', '')".ai(),
        "    return '*' * (len(digits) - 4) + digits[-4:]".ai(),
        "".ai(),
        "def format_amount(amount: float) -> str:".ai(),
        "    return f'${amount:.2f}'".ai(),
        "".ai(),
        "def generate_receipt(result: dict) -> str: return f\"Receipt: {result['transaction_id']} {result['amount']}\"".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add payment utils")
        .unwrap();

    // Rebase — C4 will conflict on service.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on service.py at C4"
    );

    // AI resolves: 25-line merged implementation (all .ai() except function signature line)
    let mut conflict_service = repo.filename("service.py");
    conflict_service.set_contents(crate::lines![
        human_context.expected_line("def process_payment(amount, card):"),
        "    from validators import validate_amount, validate_card".ai(),
        "    from exceptions import PaymentError, CardDeclinedError".ai(),
        "    from models import PaymentResult".ai(),
        "    import logging".ai(),
        "    logger = logging.getLogger(__name__)".ai(),
        "    logger.info(f'Processing: amount={amount} card=***{str(card)[-4:]}')".ai(),
        "    if amount <= 0:".ai(),
        "        raise ValueError('amount must be positive')".ai(),
        "    if not validate_amount(amount):".ai(),
        "        raise PaymentError(f'Amount out of range: {amount}')".ai(),
        "    if not validate_card(card):".ai(),
        "        raise PaymentError('Invalid card number format')".ai(),
        "    if str(card).startswith('0000'):".ai(),
        "        raise CardDeclinedError()".ai(),
        "    transaction_id = f'txn_{hash(str(card) + str(amount)) % 10**9}'".ai(),
        "    logger.info(f'Payment OK: txn={transaction_id}')".ai(),
        "    result = PaymentResult(".ai(),
        "        status='ok',".ai(),
        "        transaction_id=transaction_id,".ai(),
        "        amount=amount,".ai(),
        "    )".ai(),
        "    return {'status': result.status, 'transaction_id': result.transaction_id, 'amount': result.amount}".ai(),
        "    # AI merged: combined validation + result model".ai(),
        "    # end process_payment".ai(),
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': models.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["models.py"]);

    // C2': validators.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["validators.py"]);

    // C3': exceptions.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["exceptions.py"]);

    // C4': service.py only (AI-resolved: 24 AI lines in function body)
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["service.py"]);

    // blame at chain[3] for service.py: lines from parent (main's version) are human,
    // all new lines written by AI during conflict resolution are AI.
    assert_blame_at_commit(
        &repo,
        &chain[3],
        "service.py",
        "c4_blame_service",
        &[
            ("def process_payment", false),
            ("validate_amount, validate_card", true),
            ("PaymentError, CardDeclinedError", true),
            ("PaymentResult", true),
            ("import logging", true),
            ("logger = logging", true),
            ("Processing:", true),
            ("if amount <= 0:", false),
            ("must be positive", false),
            ("if not validate_amount", true),
            ("Amount out of range", true),
            ("if not validate_card", true),
            ("Invalid card number", true),
            ("startswith('0000')", true),
            ("CardDeclinedError()", true),
            ("transaction_id = ", true),
            ("Payment OK:", true),
            ("result = PaymentResult(", true),
            ("status='ok',", true),
            ("transaction_id=transaction_id,", true),
            ("amount=amount,", true),
            (")", true),
            ("return {", true),
            ("AI merged", true),
            ("end process_payment", true),
        ],
    );

    // C5': utils.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["utils.py"]);

    human_context.assert_metadata_humans(&repo, &chain[3], "c4'");
}

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

crate::reuse_tests_in_worktree!(
    test_conflict_ai_resolves_multiple_files_in_same_commit,
    test_conflict_ai_resolves_multiple_files_in_same_commit_standard_human,
    test_conflict_ai_resolves_then_more_ai_builds_on_result,
    test_conflict_ai_resolves_then_more_ai_builds_on_result_standard_human,
    test_conflict_ai_resolves_rust_struct_fields,
    test_conflict_ai_resolves_rust_struct_fields_standard_human,
    test_conflict_ai_resolves_complex_function_with_error_handling,
    test_conflict_ai_resolves_complex_function_with_error_handling_standard_human,
    test_conflict_mixed_ai_and_human_resolve_different_commits,
);
