use super::*;

/// Test 1: Python auth.py — feature adds AI login/logout functions, main edits
/// the same file's header comment → conflict on C1.  Human resolves by keeping
/// both parts.  C1' must have NO auth.py in its note; C2'–C5' accumulate other
/// AI files (models.py, views.py, serializers.py, signals.py) normally.
#[test]
fn test_human_conflict_python_auth_c1_conflicts_rest_accumulate() {
    let repo = TestRepo::new();

    // Initial: auth.py with a single line
    repo.commit_untracked_file("auth.py", "# auth module\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: edit auth.py header → will conflict with feature's C1
    repo.commit_untracked_file(
        "auth.py",
        "# authentication module — production\n",
        "main: update auth.py header",
    );
    repo.commit_untracked_file(
        "middleware.py",
        "class AuthMiddleware: pass\n",
        "main: add middleware",
    );
    repo.commit_untracked_file(
        "permissions.py",
        "class IsAuthenticated: pass\n",
        "main: add permissions",
    );
    repo.commit_untracked_file(
        "tokens.py",
        "import secrets\ndef generate_token(): return secrets.token_hex(32)\n",
        "main: add tokens",
    );
    repo.commit_untracked_file(
        "urls.py",
        "from django.urls import path\nurlpatterns = []\n",
        "main: add urls",
    );

    // Feature branch from initial commit (before main's auth.py edit)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI adds login/logout to auth.py — WILL CONFLICT with main's header change
    let mut auth = repo.filename("auth.py");
    auth.set_contents(crate::lines![
        "# auth module",
        "".ai(),
        "def login(username: str, password: str) -> bool:".ai(),
        "    \"\"\"Authenticate user credentials.\"\"\"".ai(),
        "    return username == 'admin' and password == 'secret'".ai(),
        "".ai(),
        "def logout(session_id: str) -> None:".ai(),
        "    \"\"\"Invalidate the given session.\"\"\"".ai(),
        "    pass".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add login and logout to auth.py")
        .unwrap();

    // C2: AI creates models.py
    let mut models = repo.filename("models.py");
    models.set_contents(crate::lines![
        "from dataclasses import dataclass".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class User:".ai(),
        "    id: int".ai(),
        "    username: str".ai(),
        "    email: str".ai(),
        "    is_active: bool = True".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add User model")
        .unwrap();

    // C3: AI creates views.py
    let mut views = repo.filename("views.py");
    views.set_contents(crate::lines![
        "from .auth import login, logout".ai(),
        "".ai(),
        "def login_view(request):".ai(),
        "    ok = login(request.POST['username'], request.POST['password'])".ai(),
        "    return {'ok': ok}".ai(),
        "".ai(),
        "def logout_view(request):".ai(),
        "    logout(request.session['id'])".ai(),
        "    return {'ok': True}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add login/logout views")
        .unwrap();

    // C4: AI creates serializers.py
    let mut serializers = repo.filename("serializers.py");
    serializers.set_contents(crate::lines![
        "class UserSerializer:".ai(),
        "    fields = ['id', 'username', 'email']".ai(),
        "".ai(),
        "    def serialize(self, user) -> dict:".ai(),
        "        return {f: getattr(user, f) for f in self.fields}".ai(),
        "".ai(),
        "    def deserialize(self, data: dict):".ai(),
        "        return data".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add UserSerializer")
        .unwrap();

    // C5: AI creates signals.py
    let mut signals = repo.filename("signals.py");
    signals.set_contents(crate::lines![
        "from typing import Callable".ai(),
        "".ai(),
        "_handlers: list[Callable] = []".ai(),
        "".ai(),
        "def on_login(fn: Callable) -> Callable:".ai(),
        "    _handlers.append(fn)".ai(),
        "    return fn".ai(),
        "".ai(),
        "def emit_login(user) -> None:".ai(),
        "    for h in _handlers: h(user)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add login signal emitter")
        .unwrap();

    // Rebase onto main — C1 will conflict on auth.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "C1 rebase should conflict on auth.py"
    );

    // Human resolves: keep both header variants merged manually (no checkpoint)
    fs::write(
        repo.path().join("auth.py"),
        "# authentication module — production\n\ndef login(username: str, password: str) -> bool:\n    return username == 'admin' and password == 'secret'\n\ndef logout(session_id: str) -> None:\n    pass\n",
    ).unwrap();
    repo.git(&["add", "auth.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    // Collect rebased chain [C1', C2', C3', C4', C5']
    let chain = get_commit_chain(&repo, 5);

    // C1': human resolved auth.py — AI content survived resolution → auth.py IS in note
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["auth.py"]);

    // C2': models.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["models.py"]);

    // C3': views.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["views.py"]);

    // C4': serializers.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["serializers.py"]);

    // C5': signals.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["signals.py"]);
}

/// Test 2: Rust lib.rs — feature adds AI parser functions, main edits the same
/// mod declaration at the top → conflict on C2 (middle of chain).
/// C1' is attributed normally; C2' loses lib.rs; C3'–C5' accumulate helpers.rs,
/// types.rs, error.rs as expected.
#[test]
fn test_human_conflict_rust_lib_c2_conflicts_surroundings_ok() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("src/lib.rs", "pub mod parser;\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: changes the mod declaration → conflicts with feature C2's edit
    repo.commit_untracked_file(
        "src/lib.rs",
        "pub mod parser;\npub mod types;\n",
        "main: add types mod",
    );
    repo.commit_untracked_file("src/main.rs", "fn main() {}\n", "main: add main.rs");
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "main: add Cargo.toml",
    );
    repo.commit_untracked_file(
        "README.md",
        "# mylib\nA Rust library.\n",
        "main: add README",
    );
    repo.commit_untracked_file(".github/workflows/ci.yml",
        "on: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps: [{uses: actions/checkout@v3}]\n",
        "main: add CI workflow",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI adds parser/tokenize to a separate file
    let mut tokenizer = repo.filename("src/tokenizer.rs");
    tokenizer.set_contents(crate::lines![
        "pub enum Token { Ident(String), Number(i64), Eof }".ai(),
        "".ai(),
        "pub fn tokenize(input: &str) -> Vec<Token> {".ai(),
        "    input.split_whitespace()".ai(),
        "        .map(|w| Token::Ident(w.to_string()))".ai(),
        "        .collect()".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add tokenizer").unwrap();

    // C2: AI edits lib.rs to export tokenizer — WILL CONFLICT with main's mod change
    let mut lib = repo.filename("src/lib.rs");
    lib.replace_at(0, "pub mod tokenizer;".ai());
    repo.stage_all_and_commit("feat: C2 export tokenizer in lib.rs")
        .unwrap();

    // C3: AI adds helpers.rs
    let mut helpers = repo.filename("src/helpers.rs");
    helpers.set_contents(crate::lines![
        "pub fn is_digit(c: char) -> bool { c.is_ascii_digit() }".ai(),
        "pub fn is_alpha(c: char) -> bool { c.is_alphabetic() }".ai(),
        "pub fn is_whitespace(c: char) -> bool { c.is_whitespace() }".ai(),
        "pub fn to_lowercase(s: &str) -> String { s.to_lowercase() }".ai(),
        "pub fn trim_quotes(s: &str) -> &str { s.trim_matches('\"') }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add helpers").unwrap();

    // C4: AI adds types.rs
    let mut types = repo.filename("src/types.rs");
    types.set_contents(crate::lines![
        "#[derive(Debug, Clone, PartialEq)]".ai(),
        "pub struct Span { pub start: usize, pub end: usize }".ai(),
        "".ai(),
        "#[derive(Debug)]".ai(),
        "pub enum ParseError {".ai(),
        "    UnexpectedToken(String),".ai(),
        "    UnexpectedEof,".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add types").unwrap();

    // C5: AI adds error.rs
    let mut error = repo.filename("src/error.rs");
    error.set_contents(crate::lines![
        "use std::fmt;".ai(),
        "use crate::types::ParseError;".ai(),
        "".ai(),
        "impl fmt::Display for ParseError {".ai(),
        "    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {".ai(),
        "        match self {".ai(),
        "            ParseError::UnexpectedToken(t) => write!(f, \"unexpected: {}\", t),".ai(),
        "            ParseError::UnexpectedEof => write!(f, \"unexpected EOF\"),".ai(),
        "        }".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add error Display impl")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/lib.rs at C2"
    );

    // Human resolves by keeping both mods
    fs::write(
        repo.path().join("src/lib.rs"),
        "pub mod parser;\npub mod types;\npub mod tokenizer;\n",
    )
    .unwrap();
    repo.git(&["add", "src/lib.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': tokenizer.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/tokenizer.rs"]);

    // C2': lib.rs human-resolved conflict — all AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &[]);

    // C3': helpers.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/helpers.rs"]);

    // C4': types.rs only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/types.rs"]);

    // C5': error.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/error.rs"]);
}

/// Test 3: TypeScript api.ts — feature adds AI REST handlers, main adds an
/// import at the top that conflicts with feature's C3.  C1'–C2' accumulate
/// dto.ts and service.ts; C3' loses api.ts attribution; C4'–C5' add more files.
#[test]
fn test_human_conflict_typescript_api_c3_conflicts_accumulation_intact() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "src/api.ts",
        "// api module\nexport {};\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: replaces the export line in api.ts — conflicts with feature's C3 which also replaces it
    repo.commit_untracked_file(
        "src/api.ts",
        "// api module\nexport { version };\n",
        "main: export version",
    );
    repo.commit_untracked_file(
        "src/server.ts",
        "import express from 'express';\nconst app = express();\napp.listen(3000);\n",
        "main: add server",
    );
    repo.commit_untracked_file(
        "src/config.ts",
        "export const PORT = parseInt(process.env.PORT ?? '3000', 10);\n",
        "main: add config",
    );
    repo.commit_untracked_file(
        "src/logger.ts",
        "export const log = (msg: string) => console.log(`[LOG] ${msg}`);\n",
        "main: add logger",
    );
    repo.commit_untracked_file(
        "tsconfig.json",
        "{\"compilerOptions\":{\"target\":\"ES2020\",\"module\":\"commonjs\",\"strict\":true}}\n",
        "main: add tsconfig",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates dto.ts
    let mut dto = repo.filename("src/dto.ts");
    dto.set_contents(crate::lines![
        "export interface CreateUserDto {".ai(),
        "  name: string;".ai(),
        "  email: string;".ai(),
        "  password: string;".ai(),
        "}".ai(),
        "".ai(),
        "export interface UpdateUserDto {".ai(),
        "  name?: string;".ai(),
        "  email?: string;".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add user DTOs").unwrap();

    // C2: AI creates service.ts
    let mut service = repo.filename("src/service.ts");
    service.set_contents(crate::lines![
        "import { CreateUserDto, UpdateUserDto } from './dto';".ai(),
        "const users: Map<number, any> = new Map();".ai(),
        "let nextId = 1;".ai(),
        "export const createUser = (dto: CreateUserDto) => { const u = { id: nextId++, ...dto }; users.set(u.id, u); return u; };".ai(),
        "export const getUser = (id: number) => users.get(id);".ai(),
        "export const updateUser = (id: number, dto: UpdateUserDto) => { const u = users.get(id); if (u) Object.assign(u, dto); return u; };".ai(),
        "export const deleteUser = (id: number) => users.delete(id);".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add user service")
        .unwrap();

    // C3: AI edits api.ts to add route handlers — WILL CONFLICT with main's express import
    let mut api = repo.filename("src/api.ts");
    api.replace_at(1, "import { createUser, getUser } from './service';".ai());
    repo.stage_all_and_commit("feat: C3 add route imports to api.ts")
        .unwrap();

    // C4: AI creates middleware.ts
    let mut mw = repo.filename("src/middleware.ts");
    mw.set_contents(crate::lines![
        "import { Request, Response, NextFunction } from 'express';".ai(),
        "".ai(),
        "export const errorHandler = (err: Error, _req: Request, res: Response, _next: NextFunction) => {".ai(),
        "  console.error(err.stack);".ai(),
        "  res.status(500).json({ error: err.message });".ai(),
        "};".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add error middleware")
        .unwrap();

    // C5: AI creates validators.ts
    let mut validators = repo.filename("src/validators.ts");
    validators.set_contents(crate::lines![
        "export const isEmail = (s: string) => /^[^@]+@[^@]+\\.[^@]+$/.test(s);".ai(),
        "export const isNonEmpty = (s: string) => s.trim().length > 0;".ai(),
        "export const isPositiveInt = (n: number) => Number.isInteger(n) && n > 0;".ai(),
        "export const clamp = (n: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, n));".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add validators")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/api.ts at C3"
    );

    // Human resolves: keep both the export and the new import
    fs::write(
        repo.path().join("src/api.ts"),
        "// api module\nexport { version };\nimport { createUser, getUser } from './service';\n",
    )
    .unwrap();
    repo.git(&["add", "src/api.ts"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': dto.ts only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/dto.ts"]);

    // C2': service.ts only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/service.ts"]);

    // C3': api.ts human-resolved conflict — AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &[]);

    // C4': middleware.ts only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/middleware.ts"]);

    // C5': validators.ts only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/validators.ts"]);
}

/// Test 4: Python models.py — main adds a class attribute that conflicts with
/// feature's last commit (C5).  All prior AI commits C1'–C4' are attributed
/// normally; C5' loses models.py.
#[test]
fn test_human_conflict_python_models_c5_last_commit_conflicts() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("models.py", "class User:\n    pass\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: adds a class attribute to models.py — conflicts with C5's edit
    repo.commit_untracked_file(
        "models.py",
        "class User:\n    table_name = 'users'\n    pass\n",
        "main: add table_name attribute",
    );
    repo.commit_untracked_file(
        "db.py",
        "import sqlite3\nconn = sqlite3.connect(':memory:')\n",
        "main: add db",
    );
    repo.commit_untracked_file("migrations/__init__.py", "", "main: add migrations package");
    repo.commit_untracked_file(
        "schema.sql",
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\n",
        "main: add schema",
    );
    repo.commit_untracked_file("seeds.py", "def seed(): pass\n", "main: add seeds");

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates repository.py
    let mut repo_file = repo.filename("repository.py");
    repo_file.set_contents(crate::lines![
        "from typing import Optional, List".ai(),
        "from .models import User".ai(),
        "".ai(),
        "class UserRepository:".ai(),
        "    def __init__(self): self._store: List[User] = []".ai(),
        "    def save(self, u: User): self._store.append(u)".ai(),
        "    def find(self, id: int) -> Optional[User]: return next((u for u in self._store if u.id == id), None)".ai(),
        "    def all(self) -> List[User]: return list(self._store)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add UserRepository")
        .unwrap();

    // C2: AI creates query_builder.py
    let mut qb = repo.filename("query_builder.py");
    qb.set_contents(crate::lines![
        "class QueryBuilder:".ai(),
        "    def __init__(self, table: str): self.table = table; self._filters: list = []".ai(),
        "    def where(self, **kw): self._filters.append(kw); return self".ai(),
        "    def build(self) -> str:".ai(),
        "        clauses = ' AND '.join(f\"{k}='{v}'\" for d in self._filters for k, v in d.items())".ai(),
        "        return f'SELECT * FROM {self.table}' + (f' WHERE {clauses}' if clauses else '')".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add QueryBuilder")
        .unwrap();

    // C3: AI creates validators.py
    let mut val = repo.filename("validators.py");
    val.set_contents(crate::lines![
        "def validate_user(data: dict) -> list[str]:".ai(),
        "    errors = []".ai(),
        "    if not data.get('name'): errors.append('name required')".ai(),
        "    if not data.get('email') or '@' not in data['email']: errors.append('valid email required')".ai(),
        "    if len(data.get('password', '')) < 8: errors.append('password min 8 chars')".ai(),
        "    return errors".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add validate_user")
        .unwrap();

    // C4: AI creates events.py
    let mut events = repo.filename("events.py");
    events.set_contents(crate::lines![
        "from typing import Callable, Dict, List".ai(),
        "_subs: Dict[str, List[Callable]] = {}".ai(),
        "def subscribe(event: str, fn: Callable): _subs.setdefault(event, []).append(fn)".ai(),
        "def publish(event: str, **data): [fn(**data) for fn in _subs.get(event, [])]".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add event pub/sub")
        .unwrap();

    // C5: AI edits models.py to add validator — WILL CONFLICT with main's table_name
    let mut models = repo.filename("models.py");
    models.replace_at(
        1,
        "    def validate(self): return bool(getattr(self, 'name', None))".ai(),
    );
    repo.stage_all_and_commit("feat: C5 add validate method to User")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on models.py at C5"
    );

    // Human resolves by keeping all three lines
    fs::write(
        repo.path().join("models.py"),
        "class User:\n    table_name = 'users'\n    def validate(self): return bool(getattr(self, 'name', None))\n    pass\n",
    ).unwrap();
    repo.git(&["add", "models.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': repository.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["repository.py"]);

    // C2': query_builder.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["query_builder.py"]);

    // C3': validators.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["validators.py"]);

    // C4': events.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["events.py"]);

    // C5': models.py human-resolved conflict — AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &[]);
}

/// Test 5: Rust src/config.rs — main and feature both extend a constants block,
/// triggering a conflict on C2.  C1' has config.rs attributed; C2' loses it
/// due to human resolution; C3'–C5' accumulate cache.rs, retry.rs, timeout.rs.
#[test]
fn test_human_conflict_rust_config_c2_loses_attribution_rest_accumulate() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "src/config.rs",
        "pub const MAX_CONN: u32 = 10;\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: adds another constant → conflicts with feature's C2 edit
    repo.commit_untracked_file(
        "src/config.rs",
        "pub const MAX_CONN: u32 = 10;\npub const TIMEOUT_MS: u64 = 5000;\n",
        "main: add TIMEOUT_MS constant",
    );
    repo.commit_untracked_file("src/pool.rs",
        "pub struct Pool { size: u32 }\nimpl Pool { pub fn new(size: u32) -> Self { Pool { size } } }\n",
        "main: add connection pool",
    );
    repo.commit_untracked_file(
        "src/metrics.rs",
        "pub fn record_latency(ms: u64) { eprintln!(\"latency: {}ms\", ms); }\n",
        "main: add metrics",
    );
    repo.commit_untracked_file(
        "src/health.rs",
        "pub fn is_healthy() -> bool { true }\n",
        "main: add health check",
    );
    repo.commit_untracked_file(
        "src/shutdown.rs",
        "pub fn graceful_shutdown() { eprintln!(\"shutting down\"); }\n",
        "main: add shutdown handler",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates a separate file (no conflict — config.rs not touched)
    let mut defaults = repo.filename("src/defaults.rs");
    defaults.set_contents(crate::lines!["pub const DEFAULT_POOL_SIZE: u32 = 5;".ai(),]);
    repo.stage_all_and_commit("feat: C1 add defaults.rs")
        .unwrap();

    // C2: AI edits config.rs to add IDLE_TIMEOUT — WILL CONFLICT with main's TIMEOUT_MS
    let mut config = repo.filename("src/config.rs");
    config.set_contents(crate::lines![
        "pub const MAX_CONN: u32 = 10;",
        "pub const IDLE_TIMEOUT_MS: u64 = 30_000;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add IDLE_TIMEOUT_MS to config")
        .unwrap();

    // C3: AI creates cache.rs
    let mut cache = repo.filename("src/cache.rs");
    cache.set_contents(crate::lines![
        "use std::collections::HashMap;".ai(),
        "pub struct Cache<K, V>(HashMap<K, V>);".ai(),
        "impl<K: Eq + std::hash::Hash, V> Cache<K, V> {".ai(),
        "    pub fn new() -> Self { Cache(HashMap::new()) }".ai(),
        "    pub fn get(&self, k: &K) -> Option<&V> { self.0.get(k) }".ai(),
        "    pub fn set(&mut self, k: K, v: V) { self.0.insert(k, v); }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add Cache struct")
        .unwrap();

    // C4: AI creates retry.rs
    let mut retry = repo.filename("src/retry.rs");
    retry.set_contents(crate::lines![
        "use crate::config::MAX_RETRIES;".ai(),
        "pub fn with_retry<T, E>(mut f: impl FnMut() -> Result<T, E>) -> Result<T, E> {".ai(),
        "    let mut last = f();".ai(),
        "    for _ in 1..MAX_RETRIES {".ai(),
        "        if last.is_ok() { return last; }".ai(),
        "        last = f();".ai(),
        "    }".ai(),
        "    last".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add retry helper")
        .unwrap();

    // C5: AI creates timeout.rs
    let mut timeout_file = repo.filename("src/timeout.rs");
    timeout_file.set_contents(crate::lines![
        "use std::time::{Duration, Instant};".ai(),
        "".ai(),
        "pub fn run_with_timeout<T>(duration: Duration, f: impl FnOnce() -> T) -> Option<T> {".ai(),
        "    let start = Instant::now();".ai(),
        "    let result = f();".ai(),
        "    if start.elapsed() <= duration { Some(result) } else { None }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add timeout runner")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/config.rs at C2"
    );

    // Human resolves: keep all constants
    fs::write(
        repo.path().join("src/config.rs"),
        "pub const MAX_CONN: u32 = 10;\npub const TIMEOUT_MS: u64 = 5000;\npub const IDLE_TIMEOUT_MS: u64 = 30_000;\n",
    ).unwrap();
    repo.git(&["add", "src/config.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': defaults.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/defaults.rs"]);

    // C2': config.rs human-resolved conflict — AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &[]);

    // C3': cache.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/cache.rs"]);

    // C4': retry.rs only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/retry.rs"]);

    // C5': timeout.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/timeout.rs"]);
}

/// Test 6: TypeScript store.ts — the entire feature file is written by the AI
/// via fs::write + git_og add + checkpoint (simulating an AI-created file).
/// Main edits the same file causing conflict on C1; human resolves.
/// No file is attributed in C1' (human resolved the only AI file in that commit).
/// C2'–C5' accumulate actions.ts, selectors.ts, reducers.ts, hooks.ts.
#[test]
fn test_human_conflict_typescript_store_ai_created_file_conflict() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "src/store.ts",
        "export const store = {};\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: modifies store.ts initial export → conflict with feature C1
    repo.commit_untracked_file(
        "src/store.ts",
        "import { createStore } from 'redux';\nexport const store = createStore(() => ({}));\n",
        "main: convert store to redux",
    );
    repo.commit_untracked_file(
        "src/index.ts",
        "export { store } from './store';\n",
        "main: re-export store",
    );
    repo.commit_untracked_file(
        "src/types.ts",
        "export type RootState = ReturnType<typeof import('./store').store.getState>;\n",
        "main: add RootState type",
    );
    repo.commit_untracked_file(
        "src/constants.ts",
        "export const ACTIONS = { INCREMENT: 'INCREMENT', DECREMENT: 'DECREMENT' } as const;\n",
        "main: add action constants",
    );
    repo.commit_untracked_file(
        "package.json",
        "{\"name\":\"app\",\"version\":\"1.0.0\",\"dependencies\":{\"redux\":\"^4.0.0\"}}\n",
        "main: add package.json",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI writes store.ts from scratch via fs::write + checkpoint
    let store_content = "import { configureStore } from '@reduxjs/toolkit';\nimport { counterSlice } from './reducers';\nexport const store = configureStore({ reducer: { counter: counterSlice.reducer } });\nexport type AppDispatch = typeof store.dispatch;\n";
    fs::write(repo.path().join("src/store.ts"), store_content).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/store.ts"])
        .unwrap();
    repo.stage_all_and_commit("feat: C1 AI rewrites store with redux toolkit")
        .unwrap();

    // C2: AI creates actions.ts
    let mut actions = repo.filename("src/actions.ts");
    actions.set_contents(crate::lines![
        "export const increment = () => ({ type: 'INCREMENT' as const });".ai(),
        "export const decrement = () => ({ type: 'DECREMENT' as const });".ai(),
        "export const reset = () => ({ type: 'RESET' as const });".ai(),
        "export type Action = ReturnType<typeof increment | typeof decrement | typeof reset>;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add action creators")
        .unwrap();

    // C3: AI creates selectors.ts
    let mut selectors = repo.filename("src/selectors.ts");
    selectors.set_contents(crate::lines![
        "import { RootState } from './types';".ai(),
        "export const selectCount = (state: RootState) => state.counter.value;".ai(),
        "export const selectIsPositive = (state: RootState) => state.counter.value > 0;".ai(),
        "export const selectIsZero = (state: RootState) => state.counter.value === 0;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add selectors").unwrap();

    // C4: AI creates reducers.ts
    let mut reducers = repo.filename("src/reducers.ts");
    reducers.set_contents(crate::lines![
        "import { createSlice } from '@reduxjs/toolkit';".ai(),
        "export const counterSlice = createSlice({".ai(),
        "  name: 'counter',".ai(),
        "  initialState: { value: 0 },".ai(),
        "  reducers: {".ai(),
        "    increment: state => { state.value += 1; },".ai(),
        "    decrement: state => { state.value -= 1; },".ai(),
        "    reset: state => { state.value = 0; },".ai(),
        "  },".ai(),
        "});".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add counter reducer")
        .unwrap();

    // C5: AI creates hooks.ts
    let mut hooks = repo.filename("src/hooks.ts");
    hooks.set_contents(crate::lines![
        "import { TypedUseSelectorHook, useDispatch, useSelector } from 'react-redux';".ai(),
        "import type { AppDispatch } from './store';".ai(),
        "import type { RootState } from './types';".ai(),
        "export const useAppDispatch = () => useDispatch<AppDispatch>();".ai(),
        "export const useAppSelector: TypedUseSelectorHook<RootState> = useSelector;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add typed hooks")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/store.ts at C1"
    );

    // Human resolves by writing a merged store file
    fs::write(
        repo.path().join("src/store.ts"),
        "import { createStore } from 'redux';\nimport { configureStore } from '@reduxjs/toolkit';\nexport const store = configureStore({ reducer: {} });\nexport type AppDispatch = typeof store.dispatch;\n",
    ).unwrap();
    repo.git(&["add", "src/store.ts"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': store.ts human-resolved → AI content survived → store.ts IS in note
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/store.ts"]);

    // C2': actions.ts only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/actions.ts"]);

    // C3': selectors.ts only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/selectors.ts"]);

    // C4': reducers.ts only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/reducers.ts"]);

    // C5': hooks.ts only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/hooks.ts"]);
}

/// Test 7: Rust src/server.rs — feature adds AI HTTP handler functions; main
/// adds a conflicting use declaration in C4.  C1'–C3' and C5' keep their AI
/// attribution; C4' (server.rs) is dropped due to human resolution.
#[test]
fn test_human_conflict_rust_server_c4_human_resolved_c5_accumulates() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("src/server.rs", "pub fn start() {}\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: adds a use statement that conflicts with feature C4's edit
    repo.commit_untracked_file(
        "src/server.rs",
        "use std::net::TcpListener;\npub fn start() {}\n",
        "main: add TcpListener import",
    );
    repo.commit_untracked_file(
        "src/router.rs",
        "pub struct Router;\nimpl Router { pub fn new() -> Self { Router } }\n",
        "main: add router",
    );
    repo.commit_untracked_file(
        "src/response.rs",
        "pub struct Response { pub status: u16, pub body: String }\n",
        "main: add Response type",
    );
    repo.commit_untracked_file(
        "src/request.rs",
        "pub struct Request { pub path: String, pub method: String }\n",
        "main: add Request type",
    );
    repo.commit_untracked_file(
        "src/middleware.rs",
        "pub trait Middleware { fn handle(&self, req: &str) -> String; }\n",
        "main: add Middleware trait",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates src/handler.rs
    let mut handler = repo.filename("src/handler.rs");
    handler.set_contents(crate::lines![
        "pub fn handle_get(path: &str) -> String {".ai(),
        "    format!(\"GET {} OK\", path)".ai(),
        "}".ai(),
        "".ai(),
        "pub fn handle_post(path: &str, body: &str) -> String {".ai(),
        "    format!(\"POST {} body={}\", path, body)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add GET/POST handlers")
        .unwrap();

    // C2: AI creates src/router_ext.rs
    let mut router_ext = repo.filename("src/router_ext.rs");
    router_ext.set_contents(crate::lines![
        "use std::collections::HashMap;".ai(),
        "pub type HandlerFn = fn(&str) -> String;".ai(),
        "pub struct RouteMap(HashMap<String, HandlerFn>);".ai(),
        "impl RouteMap {".ai(),
        "    pub fn new() -> Self { RouteMap(HashMap::new()) }".ai(),
        "    pub fn register(&mut self, path: &str, h: HandlerFn) { self.0.insert(path.to_string(), h); }".ai(),
        "    pub fn dispatch(&self, path: &str) -> Option<String> { self.0.get(path).map(|h| h(path)) }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add RouteMap").unwrap();

    // C3: AI creates src/static_files.rs
    let mut statics = repo.filename("src/static_files.rs");
    statics.set_contents(crate::lines![
        "use std::path::Path;".ai(),
        "pub fn serve_static(path: &str) -> Option<Vec<u8>> {".ai(),
        "    let p = Path::new(path);".ai(),
        "    if p.exists() { std::fs::read(p).ok() } else { None }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add static file server")
        .unwrap();

    // C4: AI edits server.rs to add bind — WILL CONFLICT with main's use std::net::TcpListener
    let mut server = repo.filename("src/server.rs");
    server.replace_at(
        0,
        "pub fn start() { let _l = std::net::TcpListener::bind(\"0.0.0.0:8080\"); }".ai(),
    );
    repo.stage_all_and_commit("feat: C4 add bind in server start")
        .unwrap();

    // C5: AI creates src/tls.rs
    let mut tls = repo.filename("src/tls.rs");
    tls.set_contents(crate::lines![
        "pub struct TlsConfig { pub cert_path: String, pub key_path: String }".ai(),
        "impl TlsConfig {".ai(),
        "    pub fn new(cert: &str, key: &str) -> Self {".ai(),
        "        TlsConfig { cert_path: cert.into(), key_path: key.into() }".ai(),
        "    }".ai(),
        "    pub fn is_valid(&self) -> bool {".ai(),
        "        std::path::Path::new(&self.cert_path).exists()".ai(),
        "            && std::path::Path::new(&self.key_path).exists()".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add TLS config struct")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/server.rs at C4"
    );

    // Human resolves by combining import and function body
    fs::write(
        repo.path().join("src/server.rs"),
        "use std::net::TcpListener;\npub fn start() { let _l = TcpListener::bind(\"0.0.0.0:8080\"); }\n",
    ).unwrap();
    repo.git(&["add", "src/server.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': handler.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/handler.rs"]);

    // C2': router_ext.rs only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/router_ext.rs"]);

    // C3': static_files.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/static_files.rs"]);

    // C4': human-resolved conflict on server.rs.  The human changed `std::net::TcpListener`
    // to `TcpListener` in the resolution — the line content differs from the original AI
    // line so content-based mapping finds no match. Note metadata is preserved but no
    // file attestations remain.
    let c4_note = repo.read_authorship_note(&chain[3]);
    assert!(
        c4_note.is_some(),
        "c4: note metadata should survive conflict rebase"
    );
    assert_note_files_exact(&repo, &chain[3], "c4_files", &[]);

    // C5': tls.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/tls.rs"]);
}

/// Test 8: Python pipeline.py — feature starts from a mixed human+AI baseline,
/// then modifies the same file as main in C3.  C1' + C2' accumulate other AI
/// files; C3' loses pipeline.py; C4'–C5' add transform.py and sink.py.
#[test]
fn test_human_conflict_python_pipeline_mixed_baseline_c3_conflict() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("pipeline.py",
        "class Pipeline:\n    def __init__(self): self.stages = []\n    def run(self, data): return data\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: adds a validate method to Pipeline — will conflict with feature C3 adding filter
    repo.commit_untracked_file("pipeline.py",
        "class Pipeline:\n    def __init__(self): self.stages = []\n    def run(self, data): return data\n    def validate(self, data): return bool(data)\n",
        "main: add Pipeline.validate",
    );
    repo.commit_untracked_file("source.py",
        "class FileSource:\n    def __init__(self, path): self.path = path\n    def read(self): return open(self.path).read()\n",
        "main: add FileSource",
    );
    repo.commit_untracked_file("registry.py",
        "_registry = {}\ndef register(name, cls): _registry[name] = cls\ndef get(name): return _registry.get(name)\n",
        "main: add component registry",
    );
    repo.commit_untracked_file("executor.py",
        "from concurrent.futures import ThreadPoolExecutor\nexec_pool = ThreadPoolExecutor(max_workers=4)\n",
        "main: add thread pool executor",
    );
    repo.commit_untracked_file("scheduler.py",
        "import sched, time\ns = sched.scheduler(time.time, time.sleep)\ndef schedule(delay, fn): s.enter(delay, 1, fn)\n",
        "main: add scheduler",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: Feature creates source.py (different name on feature branch — no conflict)
    let mut stream = repo.filename("stream.py");
    stream.set_contents(crate::lines![
        "class StreamSource:".ai(),
        "    def __init__(self, gen): self.gen = gen".ai(),
        "    def read(self): return next(self.gen, None)".ai(),
        "    def read_all(self): return list(self.gen)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add StreamSource")
        .unwrap();

    // C2: AI creates filter.py
    let mut filter_file = repo.filename("filter.py");
    filter_file.set_contents(crate::lines![
        "from typing import Callable, TypeVar".ai(),
        "T = TypeVar('T')".ai(),
        "".ai(),
        "class Filter:".ai(),
        "    def __init__(self, pred: Callable): self.pred = pred".ai(),
        "    def apply(self, data: list) -> list: return [x for x in data if self.pred(x)]".ai(),
        "    def negate(self) -> 'Filter': return Filter(lambda x: not self.pred(x))".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add Filter class")
        .unwrap();

    // C3: AI edits pipeline.py to add a filter step — WILL CONFLICT with main's validate
    let mut pipeline = repo.filename("pipeline.py");
    pipeline.replace_at(
        2,
        "    def add_filter(self, f): self.stages.append(f); return self".ai(),
    );
    repo.stage_all_and_commit("feat: C3 add Pipeline.add_filter")
        .unwrap();

    // C4: AI creates transform.py
    let mut transform = repo.filename("transform.py");
    transform.set_contents(crate::lines![
        "class Transform:".ai(),
        "    def __init__(self, fn): self.fn = fn".ai(),
        "    def apply(self, data): return [self.fn(x) for x in data]".ai(),
        "".ai(),
        "class MapTransform(Transform):".ai(),
        "    pass".ai(),
        "".ai(),
        "class FlatMapTransform:".ai(),
        "    def __init__(self, fn): self.fn = fn".ai(),
        "    def apply(self, data): return [y for x in data for y in self.fn(x)]".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add Transform classes")
        .unwrap();

    // C5: AI creates sink.py
    let mut sink = repo.filename("sink.py");
    sink.set_contents(crate::lines![
        "from typing import List".ai(),
        "".ai(),
        "class ListSink:".ai(),
        "    def __init__(self): self._items: List = []".ai(),
        "    def write(self, item): self._items.append(item)".ai(),
        "    def flush(self) -> List: r = list(self._items); self._items.clear(); return r".ai(),
        "".ai(),
        "class ConsoleSink:".ai(),
        "    def write(self, item): print(item)".ai(),
        "    def flush(self) -> List: return []".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add Sink classes")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on pipeline.py at C3"
    );

    // Human resolves by keeping both methods
    fs::write(
        repo.path().join("pipeline.py"),
        "class Pipeline:\n    def __init__(self): self.stages = []\n    def run(self, data): return data\n    def validate(self, data): return bool(data)\n    def add_filter(self, f): self.stages.append(f); return self\n",
    ).unwrap();
    repo.git(&["add", "pipeline.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': stream.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["stream.py"]);

    // C2': filter.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["filter.py"]);

    // C3': pipeline.py human-resolved conflict — AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &[]);

    // C4': transform.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["transform.py"]);

    // C5': sink.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["sink.py"]);
}

/// Test 9: TypeScript component.tsx — AI writes entire component file (via
/// fs::write + checkpoint), main adds a style import that conflicts on C2.
/// C1' accumulates hooks.ts; C2' loses component.tsx; C3'–C5' add context.ts,
/// provider.tsx, types.ts normally.
#[test]
fn test_human_conflict_typescript_component_ai_created_c2_conflict() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "src/Component.tsx",
        "export const Component = () => null;\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: adds a CSS import to Component.tsx → conflict with feature C2's rewrite
    repo.commit_untracked_file(
        "src/Component.tsx",
        "import './Component.css';\nexport const Component = () => null;\n",
        "main: add CSS import to Component",
    );
    repo.commit_untracked_file(
        "src/Component.css",
        ".component { display: flex; }\n",
        "main: add component styles",
    );
    repo.commit_untracked_file(
        "src/App.tsx",
        "import { Component } from './Component';\nexport const App = () => <Component />;\n",
        "main: add App",
    );
    repo.commit_untracked_file("src/index.tsx",
        "import React from 'react';\nimport ReactDOM from 'react-dom';\nimport { App } from './App';\nReactDOM.render(<App />, document.getElementById('root'));\n",
        "main: add entry point",
    );
    repo.commit_untracked_file(
        "src/theme.ts",
        "export const theme = { primary: '#007bff', secondary: '#6c757d' };\n",
        "main: add theme",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates hooks.ts
    let mut custom_hooks = repo.filename("src/useCounter.ts");
    custom_hooks.set_contents(crate::lines![
        "import { useState, useCallback } from 'react';".ai(),
        "".ai(),
        "export const useCounter = (initial = 0) => {".ai(),
        "  const [count, setCount] = useState(initial);".ai(),
        "  const increment = useCallback(() => setCount(c => c + 1), []);".ai(),
        "  const decrement = useCallback(() => setCount(c => c - 1), []);".ai(),
        "  const reset = useCallback(() => setCount(initial), [initial]);".ai(),
        "  return { count, increment, decrement, reset };".ai(),
        "};".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add useCounter hook")
        .unwrap();

    // C2: AI rewrites Component.tsx via fs::write + checkpoint — WILL CONFLICT
    let component_content = "import React from 'react';\nimport { useCounter } from './useCounter';\n\nexport const Component: React.FC = () => {\n  const { count, increment, decrement, reset } = useCounter();\n  return <div><button onClick={decrement}>-</button><span>{count}</span><button onClick={increment}>+</button><button onClick={reset}>reset</button></div>;\n};\n";
    fs::write(repo.path().join("src/Component.tsx"), component_content).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/Component.tsx"])
        .unwrap();
    repo.stage_all_and_commit("feat: C2 AI rewrites Component with useCounter")
        .unwrap();

    // C3: AI creates context.ts
    let mut context = repo.filename("src/context.ts");
    context.set_contents(crate::lines![
        "import React from 'react';".ai(),
        "export interface AppContextValue { theme: string; locale: string; }".ai(),
        "export const AppContext = React.createContext<AppContextValue>({ theme: 'light', locale: 'en' });".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add AppContext")
        .unwrap();

    // C4: AI creates provider.tsx
    let mut provider = repo.filename("src/provider.tsx");
    provider.set_contents(crate::lines![
        "import React, { useState } from 'react';".ai(),
        "import { AppContext } from './context';".ai(),
        "".ai(),
        "export const AppProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {".ai(),
        "  const [theme, setTheme] = useState('light');".ai(),
        "  return <AppContext.Provider value={{ theme, locale: 'en' }}>{children}</AppContext.Provider>;".ai(),
        "};".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add AppProvider")
        .unwrap();

    // C5: AI creates types.ts
    let mut types_file = repo.filename("src/types.ts");
    types_file.set_contents(crate::lines![
        "export type Theme = 'light' | 'dark';".ai(),
        "export type Locale = 'en' | 'fr' | 'de';".ai(),
        "export interface UserPrefs { theme: Theme; locale: Locale; }".ai(),
        "export type Handler<T = void> = (e: React.SyntheticEvent) => T;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add shared types")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/Component.tsx at C2"
    );

    // Human resolves: keep both CSS import and the new component body
    fs::write(
        repo.path().join("src/Component.tsx"),
        "import './Component.css';\nimport React from 'react';\nimport { useCounter } from './useCounter';\n\nexport const Component: React.FC = () => {\n  const { count, increment, decrement } = useCounter();\n  return <div>{count}</div>;\n};\n",
    ).unwrap();
    repo.git(&["add", "src/Component.tsx"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': useCounter.ts only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/useCounter.ts"]);

    // C2': Component.tsx human-resolved → AI content survived → Component.tsx IS in note
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/Component.tsx"]);

    // C3': context.ts only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/context.ts"]);

    // C4': provider.tsx only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/provider.tsx"]);

    // C5': types.ts only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/types.ts"]);
}

/// Test 10: Rust 7-commit chain — feature adds AI functions across multiple
/// files; main edits shared.rs causing conflict on C4 (middle of a 7-commit
/// chain).  Verifies 7-element chain: C1'–C3' accumulate normally, C4' loses
/// shared.rs, C5'–C7' continue accumulating math.rs, string_utils.rs, io.rs.
#[test]
fn test_human_conflict_rust_7_commit_chain_c4_conflict_surroundings_intact() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "src/shared.rs",
        "pub fn identity<T>(x: T) -> T { x }\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: adds a constant and a function to shared.rs → conflict with feature C4 edit
    repo.commit_untracked_file(
        "src/shared.rs",
        "pub const VERSION: &str = \"1.0\";\npub fn identity<T>(x: T) -> T { x }\n",
        "main: add VERSION constant to shared.rs",
    );
    repo.commit_untracked_file(
        "src/log.rs",
        "pub fn log(msg: &str) { eprintln!(\"{}\", msg); }\n",
        "main: add log",
    );
    repo.commit_untracked_file("src/env.rs",
        "pub fn env_or(key: &str, default: &str) -> String { std::env::var(key).unwrap_or_else(|_| default.to_string()) }\n",
        "main: add env helper",
    );
    repo.commit_untracked_file("src/fs_utils.rs",
        "pub fn read_to_string(path: &str) -> std::io::Result<String> { std::fs::read_to_string(path) }\n",
        "main: add fs_utils",
    );
    repo.commit_untracked_file("src/assert_utils.rs",
        "pub fn assert_non_empty(s: &str) { assert!(!s.is_empty(), \"expected non-empty string\"); }\n",
        "main: add assert_utils",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates src/vec_utils.rs
    let mut vec_utils = repo.filename("src/vec_utils.rs");
    vec_utils.set_contents(crate::lines![
        "pub fn dedup<T: Eq + std::hash::Hash + Clone>(v: &[T]) -> Vec<T> {".ai(),
        "    let mut seen = std::collections::HashSet::new();".ai(),
        "    v.iter().filter(|x| seen.insert((*x).clone())).cloned().collect()".ai(),
        "}".ai(),
        "pub fn flatten<T: Clone>(nested: &[Vec<T>]) -> Vec<T> {".ai(),
        "    nested.iter().flat_map(|v| v.iter().cloned()).collect()".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add vec_utils").unwrap();

    // C2: AI creates src/option_utils.rs
    let mut opt_utils = repo.filename("src/option_utils.rs");
    opt_utils.set_contents(crate::lines![
        "pub fn or_default<T: Default>(opt: Option<T>) -> T {".ai(),
        "    opt.unwrap_or_default()".ai(),
        "}".ai(),
        "pub fn map_or_none<T, U, F: FnOnce(T) -> Option<U>>(opt: Option<T>, f: F) -> Option<U> {"
            .ai(),
        "    opt.and_then(f)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add option_utils")
        .unwrap();

    // C3: AI creates src/result_utils.rs
    let mut result_utils = repo.filename("src/result_utils.rs");
    result_utils.set_contents(crate::lines![
        "pub fn ok_or_log<T, E: std::fmt::Display>(r: Result<T, E>, ctx: &str) -> Option<T> {".ai(),
        "    r.map_err(|e| eprintln!(\"{}: {}\", ctx, e)).ok()".ai(),
        "}".ai(),
        "pub fn map_err_string<T, E: std::fmt::Display>(r: Result<T, E>) -> Result<T, String> {"
            .ai(),
        "    r.map_err(|e| e.to_string())".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add result_utils")
        .unwrap();

    // C4: AI edits shared.rs to add a clamp function — WILL CONFLICT with main's VERSION
    let mut shared = repo.filename("src/shared.rs");
    shared.replace_at(0, "pub fn clamp<T: PartialOrd>(x: T, lo: T, hi: T) -> T { if x < lo { lo } else if x > hi { hi } else { x } }".ai());
    repo.stage_all_and_commit("feat: C4 add clamp to shared.rs")
        .unwrap();

    // C5: AI creates src/math.rs
    let mut math = repo.filename("src/math.rs");
    math.set_contents(crate::lines![
        "pub fn gcd(mut a: u64, mut b: u64) -> u64 { while b != 0 { let t = b; b = a % b; a = t; } a }".ai(),
        "pub fn lcm(a: u64, b: u64) -> u64 { a / gcd(a, b) * b }".ai(),
        "pub fn is_prime(n: u64) -> bool { if n < 2 { return false; } (2..=(n as f64).sqrt() as u64).all(|i| n % i != 0) }".ai(),
        "pub fn factorial(n: u64) -> u64 { (1..=n).product() }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add math utils")
        .unwrap();

    // C6: AI creates src/string_utils.rs
    let mut str_utils = repo.filename("src/string_utils.rs");
    str_utils.set_contents(crate::lines![
        "pub fn capitalize(s: &str) -> String {".ai(),
        "    let mut c = s.chars();".ai(),
        "    match c.next() {".ai(),
        "        None => String::new(),".ai(),
        "        Some(f) => f.to_uppercase().to_string() + c.as_str(),".ai(),
        "    }".ai(),
        "}".ai(),
        "pub fn snake_to_camel(s: &str) -> String {".ai(),
        "    s.split('_').enumerate().map(|(i, w)| if i == 0 { w.to_string() } else { capitalize(w) }).collect()".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C6 add string_utils")
        .unwrap();

    // C7: AI creates src/io_utils.rs
    let mut io_utils = repo.filename("src/io_utils.rs");
    io_utils.set_contents(crate::lines![
        "use std::io::{self, BufRead};".ai(),
        "".ai(),
        "pub fn read_lines(path: &str) -> io::Result<Vec<String>> {".ai(),
        "    let file = std::fs::File::open(path)?;".ai(),
        "    let reader = io::BufReader::new(file);".ai(),
        "    reader.lines().collect()".ai(),
        "}".ai(),
        "".ai(),
        "pub fn write_lines(path: &str, lines: &[String]) -> io::Result<()> {".ai(),
        "    std::fs::write(path, lines.join(\"\\n\"))".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C7 add io_utils").unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/shared.rs at C4"
    );

    // Human resolves: keep VERSION constant and add clamp function
    fs::write(
        repo.path().join("src/shared.rs"),
        "pub const VERSION: &str = \"1.0\";\npub fn identity<T>(x: T) -> T { x }\npub fn clamp<T: PartialOrd>(x: T, lo: T, hi: T) -> T { if x < lo { lo } else if x > hi { hi } else { x } }\n",
    ).unwrap();
    repo.git(&["add", "src/shared.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 7);

    // C1': vec_utils.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/vec_utils.rs"]);

    // C2': option_utils.rs only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/option_utils.rs"]);

    // C3': result_utils.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/result_utils.rs"]);

    // C4': shared.rs human-resolved conflict — AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &[]);

    // C5': math.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/math.rs"]);

    // C6': string_utils.rs only
    assert_note_base_commit_matches(&repo, &chain[5], "c6_base");
    assert_note_files_exact(&repo, &chain[5], "c6_files", &["src/string_utils.rs"]);

    // C7': io_utils.rs only
    assert_note_base_commit_matches(&repo, &chain[6], "c7_base");
    assert_note_files_exact(&repo, &chain[6], "c7_files", &["src/io_utils.rs"]);
}

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

/// When the ONLY AI-tracked file is the conflict file,
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

/// With three AI commits on a feature branch, the second
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
        "C1' (file_a.py, no conflict) must retain authorship note after conflict rebase"
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
        "C3' (file_c.py, no conflict) must retain authorship note after conflict rebase"
    );
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["file_c.py"]);
}

crate::reuse_tests_in_worktree!(
    test_human_conflict_python_auth_c1_conflicts_rest_accumulate,
    test_human_conflict_rust_lib_c2_conflicts_surroundings_ok,
    test_human_conflict_typescript_api_c3_conflicts_accumulation_intact,
    test_human_conflict_python_models_c5_last_commit_conflicts,
    test_human_conflict_rust_config_c2_loses_attribution_rest_accumulate,
    test_human_conflict_typescript_store_ai_created_file_conflict,
    test_human_conflict_rust_server_c4_human_resolved_c5_accumulates,
    test_human_conflict_python_pipeline_mixed_baseline_c3_conflict,
    test_human_conflict_typescript_component_ai_created_c2_conflict,
    test_human_conflict_rust_7_commit_chain_c4_conflict_surroundings_intact,
    test_human_conflict_resolves_all_ai_lines_replaced,
    test_human_conflict_ai_file_is_conflict_file_note_preserved,
    test_human_conflict_multicommit_chain_middle_conflict_all_notes_preserved,
);
